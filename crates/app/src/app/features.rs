// Laufende Logik einzelner Module: Totem-Klaenge, Voicechat, Kristall- und Ess-Optimierer.

use crate::*;

impl App {
    pub(crate) fn publish_totem_sound_enabled(&mut self) {
        self.publish_own_pop_plan();
    }

    // Baut den Plan fuer den eigenen Pop (s. toteming::OwnPopPlan) neu, sobald sich eine
    // seiner Zutaten geaendert hat. Einmal pro Tick und direkt nach jeder Aenderung im
    // Modul-Menue aufgerufen -- der Vergleich ist billig, das Aufloesen der Aufnahmen nicht.
    //
    // Vorrang: Pop-Klaenge vor dem Signalton der Totem-Anzeige vor Vanilla. Beides zugleich
    // gaebe zwei Toene fuer ein Ereignis.
    pub(crate) fn publish_own_pop_plan(&mut self) {
        let set = &self.settings;
        let key = (set.toteming.clone(), set.master_volume, set.category_volume, set.module_totem_indicator, set.module_totem_sound, self.vanilla_sounds.is_ready());
        if self.own_pop_plan_key.as_ref() == Some(&key) {
            return;
        }
        let plan = if set.toteming.sounds {
            toteming::OwnPopPlan::Samples {
                main: self.pop_voice(toteming::SoundSlot::OwnMain, 0),
                off: self.pop_voice(toteming::SoundSlot::OwnOff, 0),
            }
        } else if set.module_totem_indicator && set.module_totem_sound {
            toteming::OwnPopPlan::Tone
        } else {
            // Vanilla (ClientPlayNetworkHandler.onEntityStatus, Fall 35): item.totem.use,
            // fuer beide Haende derselbe.
            let v = self.vanilla_pop_voice();
            toteming::OwnPopPlan::Samples { main: v.clone(), off: v }
        };
        if let Ok(mut p) = self.own_pop_plan.lock() {
            *p = plan;
        }
        self.own_pop_plan_key = Some(key);
    }

    // Vanillas Pop-Klang mit Lautstaerke und Tonhoehe 1 (playSoundClient).
    pub(crate) fn vanilla_pop_voice(&mut self) -> Option<toteming::PopVoice> {
        let (sample, volume, pitch) = self.resolve_pop_sound(toteming::SOUND_VANILLA, "minecraft:item.totem.use", 0)?;
        Some(toteming::PopVoice { sample, gain: volume * self.settings.sound_gain(protocol::packets::SoundCategory::Players), pitch })
    }

    // Aufnahme, Pegel und Tonhoehe eines Klangplatzes, fertig fuer den Mixer. None = still.
    // Lautstaerke und Tonhoehe der Einstellung gelten absolut, nicht mal denen des Pakets
    // (wie in der Mod) -- nur die Lautstaerke der Kategorie kommt noch dazu.
    pub(crate) fn pop_voice(&mut self, slot: toteming::SoundSlot, seed: i64) -> Option<toteming::PopVoice> {
        let setting = self.settings.toteming.slot(slot).clone();
        let (sample, volume, pitch) = self.resolve_pop_sound(&setting.sound, slot.vanilla_event(), seed)?;
        Some(toteming::PopVoice {
            sample,
            gain: volume * setting.gain() * self.settings.sound_gain(slot.category()),
            pitch: pitch * setting.pitch_factor(),
        })
    }

    // Klangwert -> Aufnahme samt ihrer eigenen Faktoren (bei Vanilla aus sounds.json).
    pub(crate) fn resolve_pop_sound(&mut self, sound: &str, vanilla_event: &str, seed: i64) -> Option<(Arc<audio::wav::Wav>, f32, f32)> {
        if sound == toteming::SOUND_OFF {
            return None;
        }
        if sound == toteming::SOUND_VANILLA {
            return self.vanilla_sounds.resolve(&mut self.installation, vanilla_event, seed).map(|r| (r.sample, r.volume, r.pitch));
        }
        if let Some(name) = sound.strip_prefix(toteming::CUSTOM_PREFIX) {
            return self.custom_pop_sounds.get(name).map(|w| (w, 1.0, 1.0));
        }
        if sound == toteming::ORB {
            return self.vanilla_sounds.resolve(&mut self.installation, toteming::ORB_EVENT, 0).map(|r| (r.sample, r.volume, r.pitch));
        }
        self.sounds.get(sound).map(|w| (w, 1.0, 1.0))
    }

    // Ein fremder Pop: an der Stelle des Poppers, wie in Vanilla (playSoundClient an
    // entity.getX/Y/Z). Ohne Pop-Klaenge-Modul Vanillas eigener Klang.
    pub(crate) fn play_other_pop_sound(&mut self, entity_id: i32) {
        let Some(position) = self.entities.get(entity_id).map(|e| DVec3::new(e.pos.0, e.pos.1, e.pos.2)) else { return };
        let voice = if self.settings.toteming.sounds { self.pop_voice(toteming::SoundSlot::Other, 0) } else { self.vanilla_pop_voice() };
        if sound_log::enabled() {
            let ergebnis = match &voice {
                Some(v) if self.settings.toteming.sounds => format!("gespielt  pegel={:.3} (Pop-Klaenge-Modul)", v.gain),
                Some(v) => format!("gespielt  pegel={:.3}", v.gain),
                None => "stumm".to_string(),
            };
            self.log_client_sound("client", "item.totem.use", protocol::packets::SoundCategory::Players, Some(entity_id), Some(position), 1.0, 1.0, &format!("{ergebnis} (fremder Totem-Pop)"));
        }
        if let Some(v) = voice {
            let placement = self.listener().placement(position);
            self.audio.sink().play_sample_at(v.sample, v.gain, v.pitch, &placement);
        }
    }

    // Vorhoeren nach einer Aenderung im Menue -- immer kopfgebunden, wie SoundPlayer.preview.
    pub(crate) fn preview_pop_sound(&mut self, slot: toteming::SoundSlot) {
        if let Some(v) = self.pop_voice(slot, 0) {
            self.audio.sink().play_sample(v.sample, v.gain, 0.0, v.pitch);
        }
    }

    // Sprachchat beim Server anmelden: die Kanaele annehmen, auf denen er antwortet (Paper
    // liefert sonst nichts), und das Secret anfordern -- wie der Mod-Client beim Betreten der
    // Welt. Ein Server ohne Sprachchat verwirft beides.
    pub(crate) fn announce_voice_chat(&mut self) {
        let Some(tx) = &self.outgoing_tx else { return };
        let _ = tx.send(protocol::packets::register_channels_payload(&voice::packets::CLIENTBOUND_CHANNELS));
        let _ = tx.send(protocol::packets::custom_payload(voice::packets::CH_REQUEST_SECRET, &voice::packets::request_secret_payload()));
    }

    // Farbcode des Sprachchat-Zeichens am Namensschild, None = keins.
    pub(crate) fn voice_nametag_icon(&self, uuid: &[u8; 16]) -> Option<char> {
        if !self.settings.module_voice_chat || !self.settings.voice.nametag_icons || self.voice.status != voicechat::Status::Connected {
            return None;
        }
        let u = protocol::uuid::Uuid(*uuid);
        if let Some(whisper) = self.voice.talking(&u) {
            return Some(if whisper { 'b' } else { 'a' });
        }
        self.voice.states.get(uuid).filter(|st| st.disabled && !st.disconnected).map(|_| 'c')
    }

    // Modul ein- oder ausgeschaltet.
    pub(crate) fn voice_settings_changed(&mut self) {
        if self.settings.module_voice_chat {
            if self.conn_status == ConnStatus::Connected {
                self.announce_voice_chat();
            }
        } else {
            if let Some(c) = self.voice.client.as_ref().filter(|c| c.is_connected()) {
                c.stop_talking();
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::custom_payload(voice::packets::CH_UPDATE_STATE, &voice::packets::update_state_payload(true)));
                }
            }
            self.voice.reset();
        }
    }

    pub(crate) fn handle_voice_payload(&mut self, channel: &str, data: &[u8]) {
        use voice::packets as vp;
        match channel {
            vp::CH_SECRET => {
                if !self.settings.module_voice_chat {
                    return;
                }
                match vp::read_secret(data) {
                    Ok(info) => {
                        // Host ohne Port; IPv6 in eckigen Klammern.
                        let adresse = self.server_address.trim();
                        let host = match adresse.strip_prefix('[') {
                            Some(rest) => rest.split(']').next().unwrap_or(rest).to_string(),
                            None => adresse.rsplit_once(':').filter(|(h, _)| !h.contains(':')).map_or(adresse, |(h, _)| h).to_string(),
                        };
                        self.push_log(format!("Sprachchat: verbinde (Reichweite {:.0} Blöcke)", info.distance));
                        self.voice.connect(info, &host);
                        if let voicechat::Status::Failed(grund) = &self.voice.status {
                            let grund = grund.clone();
                            self.push_log(format!("Sprachchat: Verbindung fehlgeschlagen: {grund}"));
                        }
                    }
                    Err(_) => self.push_log("Sprachchat: Secret unlesbar"),
                }
            }
            vp::CH_STATE => {
                if let Ok(st) = vp::read_state(data) {
                    self.voice.states.insert(st.uuid.0, st);
                }
            }
            vp::CH_STATES => {
                if let Ok(liste) = vp::read_states(data) {
                    self.voice.states = liste.into_iter().map(|st| (st.uuid.0, st)).collect();
                }
            }
            vp::CH_REMOVE_STATE => {
                if let Ok(u) = vp::read_remove_state(data) {
                    self.voice.states.remove(&u.0);
                }
            }
            _ => {}
        }
    }

    // Ort und Pegel einer Stimme fuer den Mixer: Spieler an ihren Augen, Orte an ihrem Punkt,
    // Gruppen kopffest. Die Entfernung wirkt linear bis zur Reichweite wie bei der Mod; das
    // Ohrenmodell bekommt nur die Richtung (Entfernung 0), sonst kaeme seine eigene
    // Daempfung noch obendrauf. 3D-Klang "reduziert" halbiert die Seitenlage, "aus" legt alles
    // in die Mitte.
    pub(crate) fn voice_placement(&self, sender: &protocol::uuid::Uuid, source: &voice::SoundSource) -> (Option<audio::spatial::Placement>, f32) {
        let v = &self.settings.voice;
        let lautstaerke = v.volume_percent as f32 / 100.0;
        let ort = match source {
            voice::SoundSource::Group => None,
            voice::SoundSource::Location { pos, distance } => Some((DVec3::new(pos[0], pos[1], pos[2]), *distance)),
            voice::SoundSource::Player { distance, .. } => {
                let progress = self.tick_progress();
                self.entities
                    .iter()
                    .find(|(_, e)| e.uuid == sender.0)
                    .map(|(_, e)| (DVec3::from(e.render_pos(progress)) + DVec3::new(0.0, 1.62, 0.0), *distance))
            }
        };
        match ort {
            Some((p, reichweite)) => {
                let lage = self.listener().placement(p);
                let pegel = voicechat::distance_volume(lage.distance, reichweite);
                let seite = match v.audio_type {
                    voicechat::AudioType::Normal => Some(lage.lateral),
                    voicechat::AudioType::Reduced => Some(lage.lateral * 0.5),
                    voicechat::AudioType::Off => None,
                };
                (seite.map(|lateral| audio::spatial::Placement { lateral, distance: 0.0 }), lautstaerke * pegel)
            }
            None => (None, lautstaerke),
        }
    }

    // Eigenen Zustand an den Server melden (taub ja/nein) -- die anderen sehen ihn.
    pub(crate) fn voice_send_state(&self) {
        if self.voice.status != voicechat::Status::Connected {
            return;
        }
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::custom_payload(
                voice::packets::CH_UPDATE_STATE,
                &voice::packets::update_state_payload(self.settings.voice.disabled),
            ));
        }
    }

    pub(crate) fn voice_step(&mut self, knob: VoiceKnob, hoch: bool) {
        let v = &mut self.settings.voice;
        let schritt = |wert: i64, um: i64, min: i64, max: i64| (wert + if hoch { um } else { -um }).clamp(min, max);
        match knob {
            VoiceKnob::Volume => v.volume_percent = schritt(v.volume_percent as i64, 10, 0, voicechat::VOLUME_MAX as i64) as u32,
            VoiceKnob::MicGain => v.mic_gain_db = schritt(v.mic_gain_db as i64, 1, voicechat::GAIN_DB_MIN as i64, voicechat::GAIN_DB_MAX as i64) as i32,
            VoiceKnob::Threshold => v.threshold_db = schritt(v.threshold_db as i64, 1, voicechat::THRESHOLD_DB_MIN as i64, 0) as i32,
            VoiceKnob::Delay => {
                let feld = if v.voice_activation { &mut v.voice_delay_frames } else { &mut v.ptt_delay_frames };
                *feld = schritt(*feld as i64, 1, 0, voicechat::DELAY_MAX_FRAMES as i64) as u32;
            }
            VoiceKnob::OutputBuffer => {
                v.output_buffer_frames = schritt(v.output_buffer_frames as i64, 1, 1, voicechat::BUFFER_MAX_FRAMES as i64) as u32;
                self.audio.sink().set_stream_buffer(self.settings.voice.output_buffer_frames as usize);
            }
            VoiceKnob::PacketThreshold => {
                v.packet_threshold = schritt(v.packet_threshold as i64, 1, 0, voicechat::PACKET_THRESHOLD_MAX as i64) as u32;
                if let Some(c) = &self.voice.client {
                    c.set_packet_threshold(self.settings.voice.packet_threshold as usize);
                }
            }
        }
    }

    pub(crate) fn voice_flag(&mut self, flag: VoiceFlag) {
        let v = &mut self.settings.voice;
        match flag {
            VoiceFlag::Activation => {
                v.voice_activation = !v.voice_activation;
                self.voice.processor.reset_activation();
            }
            VoiceFlag::Agc => v.agc = !v.agc,
            VoiceFlag::Denoise => v.denoise = !v.denoise,
            VoiceFlag::AudioType => v.audio_type = v.audio_type.next(),
            VoiceFlag::MicDevice => {
                // Durch "Standard" und alle Eingabegeraete schalten; das Mikrofon oeffnet
                // voice_tick mit dem neuen Geraet neu.
                let mut liste = vec![String::new()];
                liste.extend(audio::mic::list_input_devices());
                let i = liste.iter().position(|d| *d == v.mic_device).map_or(0, |i| (i + 1) % liste.len());
                v.mic_device = liste[i].clone();
            }
            VoiceFlag::Muted => v.muted = !v.muted,
            VoiceFlag::Disabled => {
                v.disabled = !v.disabled;
                if v.disabled {
                    if let Some(c) = &self.voice.client {
                        c.stop_talking();
                    }
                    // Was gerade laeuft, verstummt sofort.
                    let sink = self.audio.sink();
                    for id in self.voice.streams.keys() {
                        sink.stream_end(*id);
                    }
                    self.voice.streams.clear();
                }
                self.voice_send_state();
            }
            VoiceFlag::MuteOnJoin => v.mute_on_join = !v.mute_on_join,
            VoiceFlag::NametagIcons => v.nametag_icons = !v.nametag_icons,
            VoiceFlag::HudIcons => v.hud_icons = !v.hud_icons,
            VoiceFlag::Monitor => {
                self.voice.monitor = !self.voice.monitor;
                if !self.voice.monitor {
                    self.audio.sink().stream_end(voicechat::MONITOR_STREAM);
                }
            }
        }
        self.persist_settings();
    }

    // Einmal pro Bild: Ereignisse der Verbindung abholen, Stimmen in den Mixer, Mikrofon
    // aufbereiten und -- per Sprechtaste oder Sprachaktivierung -- senden.
    pub(crate) fn voice_tick(&mut self) {
        let mit_mikro = self.settings.module_voice_chat && (self.voice.status == voicechat::Status::Connected || self.voice.monitor);
        if mit_mikro {
            let geraet = self.settings.voice.mic_device.clone();
            let war_zu = self.voice.mic.is_none();
            self.voice.ensure_mic(&geraet);
            if war_zu {
                if let Some(fehler) = self.voice.last_mic_error.clone() {
                    self.push_log(format!("Sprachchat: kein Mikrofon ({fehler})"));
                    // Nicht in jedem Bild erneut versuchen.
                    self.voice.monitor = false;
                }
            }
        } else if self.voice.mic.is_some() {
            self.voice.close_mic();
        }
        if self.voice.client.is_none() && self.voice.mic.is_none() {
            return;
        }
        let taub = self.settings.voice.disabled;
        for ereignis in self.voice.poll() {
            match ereignis {
                voice::VoiceEvent::Connected => {
                    self.push_log("Sprachchat: verbunden");
                    if self.settings.voice.mute_on_join && !self.settings.voice.muted {
                        self.settings.voice.muted = true;
                        self.persist_settings();
                    }
                    if let Some(c) = &self.voice.client {
                        c.set_packet_threshold(self.settings.voice.packet_threshold as usize);
                    }
                    self.audio.sink().set_stream_buffer(self.settings.voice.output_buffer_frames as usize);
                    self.voice_send_state();
                }
                voice::VoiceEvent::Disconnected(grund) => {
                    self.push_log(format!("Sprachchat: getrennt ({grund})"));
                }
                voice::VoiceEvent::Audio { .. } if taub => {}
                voice::VoiceEvent::Audio { channel, sender, source, pcm } => {
                    let id = voicechat::stream_id(&channel);
                    let sink = self.audio.sink();
                    if pcm.is_empty() {
                        sink.stream_end(id);
                        self.voice.streams.remove(&id);
                        continue;
                    }
                    let (lage, pegel) = self.voice_placement(&sender, &source);
                    sink.stream_push(id, &pcm, lage.as_ref(), pegel);
                    self.voice.streams.insert(id, voicechat::ActiveStream { sender, source, last: Instant::now() });
                }
            }
        }
        // Laufende Stimmen an Kopf und Sprecher nachfuehren; verstummte (ohne Endpaket,
        // z.B. Paketverlust) nach einer Sekunde beenden.
        let sink = self.audio.sink();
        let mut fertig = Vec::new();
        for (id, st) in &self.voice.streams {
            if st.last.elapsed() > Duration::from_secs(1) {
                sink.stream_end(*id);
                fertig.push(*id);
                continue;
            }
            let (lage, pegel) = self.voice_placement(&st.sender, &st.source);
            sink.stream_place(*id, lage.as_ref(), pegel);
        }
        for id in fertig {
            self.voice.streams.remove(&id);
        }
        // Mikrofon: jeden angefallenen Rahmen aufbereiten, messen und -- wenn aktiv -- senden.
        // Ob gesendet wird, entscheidet sich nur, wenn Rahmen da waren (das Bild laeuft
        // schneller als 50 Rahmen je Sekunde).
        let rahmen: Vec<Vec<i16>> = self.voice.mic_rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        if rahmen.is_empty() {
            return;
        }
        let ms = self.settings.voice.mic();
        let fluestern = self.whisper_held && self.chat_input.is_none();
        let taste = (self.push_to_talk_held || fluestern) && self.chat_input.is_none();
        let darf = !self.settings.voice.muted && !taub && self.voice.status == voicechat::Status::Connected;
        let mut gesendet = false;
        for mut r in rahmen {
            let pegel = self.voice.processor.process(&mut r, &ms);
            self.voice.level_db = pegel;
            let aktiv = self.voice.processor.active(&ms, taste, pegel);
            if self.voice.monitor {
                sink.stream_push(voicechat::MONITOR_STREAM, &r, None, 1.0);
            }
            if aktiv && darf {
                if let Some(c) = &self.voice.client {
                    c.send_frame(&r, fluestern);
                    gesendet = true;
                }
            }
        }
        if !gesendet && self.voice.sending {
            if let Some(c) = &self.voice.client {
                c.stop_talking();
            }
        }
        self.voice.sending = gesendet;
        self.voice.whispering = gesendet && fluestern;
    }

    // Crystal-Optimizer beim Server anmelden (ConnectEventListener.onPlayReady): den Kanal
    // fuer den Opt-out annehmen und die Version melden. Ein Server ohne Plugin verwirft beides.
    pub(crate) fn announce_crystal_optimizer(&mut self) {
        let Some(tx) = &self.outgoing_tx else { return };
        let _ = tx.send(protocol::packets::register_channels_payload(&[crystal_optimizer::CHANNEL_OPT_OUT]));
        let _ = tx.send(protocol::packets::custom_payload(crystal_optimizer::CHANNEL_VERSION, &crystal_optimizer::version_data()));
    }

    // Consumable-Optimizer beim Server anmelden (wie die Mod beim Beitreten): den Kanal fuer die
    // Abschaltung annehmen und den Handshake schicken. Ohne Plugin verwirft der Server beides.
    pub(crate) fn announce_consumable_optimizer(&mut self) {
        let Some(tx) = &self.outgoing_tx else { return };
        let _ = tx.send(protocol::packets::register_channels_payload(&[consumable_optimizer::CHANNEL_DISABLE]));
        let _ = tx.send(protocol::packets::custom_payload(consumable_optimizer::CHANNEL_HANDSHAKE, &[]));
    }

    // Fehlen beim laufenden Essen/Trinken hoechstens eat_hold_through_ms? (s. tick_item_use)
    // Beim Bogen dazu: zu frueh losgelassen haelt es bis zum ersten Pfeil, kurz vor voll bis
    // zur vollen Kraft (s. bow_hold_through).
    pub(crate) fn eat_hold_through(&self) -> bool {
        let grenze = self.settings.eat_hold_through_ms;
        if !self.settings.module_consumable_optimizer {
            return false;
        }
        let Some((name, stack)) = self.own_used_item() else { return false };
        if name == "bow" {
            return bow_hold_through(self.own_use_ticks, grenze);
        }
        if grenze == 0 {
            return false;
        }
        let Some(dauer) = consumable_optimizer::consume_ticks(name, stack.as_ref()) else { return false };
        eat_rest_within(dauer, self.own_use_ticks, grenze)
    }

    // Drueckt das Modul gerade durch (Taste oben, Rest unter der Grenze) oder hat es dieses
    // Essen durchgedrueckt und der Server ist noch nicht fertig? Ein Slotwechsel, Tausch oder
    // Wegwerfen in dieser Zeit braeche das Essen beim Server ab; mit eat_slot_lock warten sie
    // deshalb. Vorher -- solange man selbst noch drueckt -- bricht ein Wechsel das Essen ab
    // wie in Vanilla.
    pub(crate) fn server_still_eating(&self) -> bool {
        if !self.settings.eat_slot_lock {
            return false;
        }
        let hilft_jetzt = self.using_item && !self.use_held && self.eat_hold_through();
        hilft_jetzt || (self.eat_held_through && (self.using_item || self.consumable.waiting(Instant::now())))
    }

    // Modul Delay: die Verzoegerung an die Netzschicht geben. Sie greift nur auf erlaubten
    // Servern (s. net::lag), dort aber auch mitten in der Verbindung.
    pub(crate) fn sync_lag_delay(&self) {
        net::set_lag_delay(if self.settings.module_delay { self.settings.lag_delay_ms } else { 0 });
    }

    pub(crate) fn consumable_in_hand(&self) -> bool {
        [protocol::packets::HAND_MAIN, protocol::packets::HAND_OFF].iter().any(|&h| {
            let stack = self.own_hand_stack(h);
            stack
                .and_then(|st| world::item_registry::item_name(st.item_id))
                .is_some_and(|name| consumable_optimizer::consume_ticks(name, stack).is_some())
        })
    }

    pub(crate) fn consumable_optimizer_active(&self) -> bool {
        self.settings.module_consumable_optimizer && !self.consumable.disabled_by_server
    }

    // Was gerade benutzt wird: (Item-Name, Stapel), None ohne laufende Benutzung.
    pub(crate) fn own_used_item(&self) -> Option<(&'static str, Option<protocol::item::ItemStack>)> {
        if !self.using_item {
            return None;
        }
        let hand = if self.using_offhand { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
        let stack = self.own_hand_stack(hand).cloned();
        let name = world::item_registry::item_name(stack.as_ref()?.item_id)?;
        Some((name, stack))
    }

    // Der Server meldet, dass das eigene Essen fertig ist (Flag "benutzt Item" aus oder Entity-
    // Event 9 -- je nach Server kommt nur eins davon): nicht mehr warten.
    pub(crate) fn consumable_server_done(&mut self, woher: &str) {
        if let Some(d) = self.consumable.confirm_delay(Instant::now()) {
            eprintln!("[essen] lokal fertig, Server bestaetigt ({woher}) nach {} ms -- so lange blieb die Ess-Verlangsamung nach dem Klang", d.as_millis());
        }
        self.consumable.stop_waiting();
        // Er ist fertig: der naechste Bissen muss nicht mehr warten.
        self.eat_next_use_delay = 0;
    }

    // Aggressive Prediction: das Server-Flag beendet die eigene Benutzung nicht mehr, also muss
    // der Client es selbst tun, wenn das Item in der Hand wechselt -- wie Vanillas
    // LivingEntity.updatingUsingItem (anderes Item als useItem -> stopUsingItem). Sonst bliebe
    // nach einem Slotwechsel beim Spannen die Verlangsamung bis zum Loslassen stehen.
    pub(crate) fn tick_aggressive_use_cancel(&mut self) {
        if !self.using_item || !self.settings.eat_aggressive_prediction || !self.consumable_optimizer_active() {
            return;
        }
        let hand = if self.using_offhand { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
        let jetzt = self.own_hand_stack(hand).map(|st| st.item_id);
        if jetzt.is_none() || jetzt != self.using_item_id {
            self.using_item = false;
            self.own_use_ticks = 0;
        }
    }

    // Consumable Optimizer: im Tick, in dem das Essen fertig ist, selbst beenden (s.
    // consumable_optimizer.rs). Laeuft nach dem Hochzaehlen von own_use_ticks.
    pub(crate) fn tick_consumable_optimizer(&mut self) {
        if self.consumable.timed_out(Instant::now()) {
            eprintln!("[essen] lokal fertig, keine Bestaetigung vom Server -- Ess-Verlangsamung lief die vollen 500 ms nach dem Klang");
        }
        if !self.consumable_optimizer_active() {
            return;
        }
        let Some((name, stack)) = self.own_used_item() else { return };
        let Some(dauer) = consumable_optimizer::consume_ticks(name, stack.as_ref()) else { return };
        if self.own_use_ticks < dauer {
            return;
        }
        self.own_eating_particles(true);
        let klang = consumable_optimizer::finish_sound(name);
        let mut klaenge = vec![klang];
        let mut zufall = || {
            self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
            (zufallswert(self.type_pitch_seed) + 1.0) * 0.5
        };
        // Consumable.onConsume: Klang mit Tonhoehe um 1 (Dreiecksverteilung +-0.4); der
        // Ruelpser leiser, Tonhoehe 0.9 bis 1.0.
        let tonhoehe = 1.0 + 0.4 * (zufall() - zufall());
        let ruelps_hoehe = 0.9 + 0.1 * zufall();
        let auge = self.physics.eye_pos();
        self.play_event_at_scaled(klang, auge, 1.0, tonhoehe, protocol::packets::SoundCategory::Players);
        if consumable_optimizer::burps(name) {
            self.play_event_at_scaled(consumable_optimizer::BURP_SOUND, auge, 0.5, ruelps_hoehe, protocol::packets::SoundCategory::Players);
            klaenge.push(consumable_optimizer::BURP_SOUND);
        }
        self.consumable.finished_locally(&klaenge, Instant::now());
        self.eat_next_use_delay = EAT_NEXT_USE_DELAY;
        // Wie stopUsingItem im Client: kein RELEASE an den Server -- der beendet das Essen im
        // selben Tick selbst.
        self.using_item = false;
        self.own_use_ticks = 0;
    }

    // Der Server will keinen Crystal-Optimizer (OptOutPacketListener): abschalten, merken,
    // bestaetigen und -- einmal je Server -- nach zwei Sekunden im Chat darauf hinweisen.
    pub(crate) fn handle_crystal_opt_out(&mut self) {
        self.crystal_opted_out = true;
        let key = self.server_address.clone();
        if !self.crystal_opt_out_servers.contains(&key) {
            self.crystal_opt_out_servers.push(key.clone());
            // EvictingList(10): der aelteste faellt raus.
            if self.crystal_opt_out_servers.len() > 10 {
                self.crystal_opt_out_servers.remove(0);
            }
        }
        if !self.crystal_opt_out_notified.contains(&key) {
            self.crystal_opt_out_notice_at = Some(Instant::now() + Duration::from_secs(2));
        }
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::custom_payload(crystal_optimizer::CHANNEL_OPT_OUT_ACK, &[]));
        }
        self.push_log("Crystal-Optimizer: vom Server abgeschaltet");
    }

    // Der verzoegerte Hinweis aus handle_crystal_opt_out, im Wortlaut der Mod.
    pub(crate) fn tick_crystal_opt_out_notice(&mut self) {
        let Some(at) = self.crystal_opt_out_notice_at else { return };
        if Instant::now() < at || self.conn_status != ConnStatus::Connected {
            return;
        }
        self.crystal_opt_out_notice_at = None;
        let key = self.server_address.clone();
        if self.crystal_opt_out_notified.contains(&key) {
            return;
        }
        self.crystal_opt_out_notified.push(key);
        if self.crystal_opt_out_notified.len() > 10 {
            self.crystal_opt_out_notified.remove(0);
        }
        let span = |text: &str, color: u32| protocol::nbt::TextSpan { text: text.to_string(), color: Some(color), link: None, action: None, style: 0, shadow: None };
        self.push_chat_message(vec![
            span("[", 0xAAAAAA),
            span("Marlow's Crystal Optimizer", 0x55FFFF),
            span("] ", 0xAAAAAA),
            span("Optimizer disabled on this server.", 0xFF5555),
        ]);
    }

    // Pop-Zaehler (TotemPopTracker der Mod): wer wie oft in dieser Verbindung gepoppt hat.
    pub(crate) fn count_pop(&mut self, entity_id: i32) {
        let name = if self.own_entity_id == Some(entity_id) {
            "Du".to_string()
        } else {
            match self.entities.get(entity_id) {
                Some(e) => self
                    .player_names
                    .get(&e.uuid)
                    .cloned()
                    .unwrap_or_else(|| world::entity_type_name(e.entity_type).unwrap_or("Entity").to_string()),
                None => format!("Entity {entity_id}"),
            }
        };
        let n = self.pop_counts.entry(name.clone()).or_insert(0);
        *n += 1;
        let n = *n;
        self.push_log(format!("{name} hat gepoppt ({n}x)"));
    }
}
