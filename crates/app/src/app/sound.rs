// Klaenge: UI, Welt, Server, Schritte und Bloecke.

use crate::*;

impl App {
    // Einen Oberflaechen-Klang abspielen (s. ui_sound). Geht ueber denselben Mixer wie alles
    // andere; UI-Ereignisse entstehen ohnehin im Hauptthread, ein Umweg ueber den
    // Netzwerk-Haken waere hier sinnlos.
    // Spielt die Aufnahme, die `sample()` nennt (aus dem Sound-Baukasten); ohne Aufnahme
    // bleibt der Klang stumm -- kein Zurueckfallen auf die synthetischen Klaenge (`cue()`), die
    // waren verworfen ("die hoeren sich ehrlich gesagt nicht so gut an").
    pub(crate) fn play_ui_sound(&mut self, sound: ui_sound::UiSound) {
        self.play_ui_sound_pitched(sound, 0.0);
    }

    // Dasselbe um `halbtoene` verschoben (Slider-Tick: steigt mit dem Wert). Je Abspielen wird
    // die Streuung des Klangs gewuerfelt; kommt derselbe Klang schneller als sein
    // Mindestabstand, wird er geschluckt (s. ui_sound::UiSound::min_gap).
    pub(crate) fn play_ui_sound_pitched(&mut self, sound: ui_sound::UiSound, halbtoene: f32) {
        let i = sound.index();
        let jetzt = Instant::now();
        if self.ui_sound_last[i].is_some_and(|t| jetzt.duration_since(t) < sound.min_gap()) {
            return;
        }
        let Some(aufnahme) = sound.sample().and_then(|name| self.sounds.get(name)) else { return };
        self.ui_sound_last[i] = Some(jetzt);
        let streuung = sound.spread_semitones();
        let zufall = if streuung > 0.0 {
            self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
            zufallswert(self.type_pitch_seed) * streuung
        } else {
            0.0
        };
        // Der Tonhoehen-Regler im Audio-Tab gilt fuer alle Klaenge ausser dem Tippen: das soll
        // in jeder Config gleich klingen.
        let regler = if sound == ui_sound::UiSound::Type { 0.0 } else { self.settings.ui_pitch_semitones as f32 };
        let pitch = audio::semitone_factor(regler + halbtoene + zufall);
        let einzeln = self.settings.ui_sound_volume.get(i).copied().unwrap_or(100.0) / 100.0;
        let gain = sound.sample_gain() * self.settings.sound_gain(protocol::packets::SoundCategory::Ui) * einzeln;
        sound_log::sound("ui", &format!("{sound:?}"), protocol::packets::SoundCategory::Ui, "-", None, 1.0, pitch, &format!("gespielt  pegel={gain:.3}"));
        self.audio.sink().play_sample(aufnahme, gain, 0.0, pitch);
    }

    // Slider-Tick bei Wert `t` (0 = linkes, 1 = rechtes Ende der Bahn).
    pub(crate) fn play_slider_tick(&mut self, t: f64) {
        self.play_ui_sound_pitched(ui_sound::UiSound::Slider, t.clamp(0.0, 1.0) as f32 * SLIDER_TICK_RISE_SEMITONES);
    }

    // Wo sitzen die Ohren? Am Auge des Spielers, ausgerichtet an SEINER Blickrichtung --
    // nicht an der Kamera, s. spatial_sound::Listener::at_eye.
    pub(crate) fn listener(&self) -> spatial_sound::Listener {
        spatial_sound::Listener::at_eye(self.physics.eye_pos(), self.camera.yaw)
    }

    /// Eine Aufnahme an einem ORT abspielen -- der Weg fuer alles, was in der Welt passiert.
    ///
    /// Absichtlich hier und nicht im Tick: dieselbe Regel wie fuer die UI-Klaenge -- ein Ton
    /// ist ein Reaktionssignal, und ein Tick sind 50 ms. Wer diese Funktion aus der
    /// Tick-Schleife aufruft, macht den Ton bis zu eine Zwanzigstelsekunde zu spaet.
    pub(crate) fn play_world_sound(&mut self, name: &str, position: DVec3, gain: f32, pitch: f32) {
        let Some(aufnahme) = self.sounds.get(name) else { return };
        let placement = self.listener().placement(position);
        self.audio.sink().play_sample_at(aufnahme, gain, pitch, &placement);
    }

    // Ein Schild wurde deaktiviert (BlocksAttacksComponent.applyShieldCooldown spielt dann den
    // disabled_sound am Getroffenen ab). Den Cooldown selbst bekommt nur der Getroffene --
    // fuer alle anderen ist der Klang das einzige Zeichen. Er gehoert dem Spieler, der am
    // naechsten am Klangort steht; ist das man selbst, fuehrt ohnehin item_cooldowns Buch.
    pub(crate) fn note_shield_break(&mut self, sound: &protocol::packets::SoundEffect) {
        let ort = match (sound.position, sound.entity_id) {
            (Some((x, y, z)), _) => DVec3::new(x, y, z),
            (None, Some(id)) => {
                if self.entities.get(id).is_some_and(|e| world::is_player(e.entity_type)) {
                    self.shield_cooldowns.insert(id, ShieldCooldown::new());
                }
                return;
            }
            (None, None) => return,
        };
        let eigener = self.physics.pos.distance(ort);
        let naechster = self
            .entities
            .iter()
            .filter(|(_, e)| world::is_player(e.entity_type))
            // Beide Positionen: die geglaettete hinkt der des Servers (dort entstand der Klang)
            // bei schneller Bewegung hinterher.
            .map(|(id, e)| {
                let (p, t) = (e.pos, e.target_pos());
                (*id, DVec3::new(p.0, p.1, p.2).distance(ort).min(DVec3::new(t.0, t.1, t.2).distance(ort)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, abstand)) = naechster {
            if abstand <= SHIELD_BREAK_MATCH_RADIUS && abstand < eigener {
                self.shield_cooldowns.insert(id, ShieldCooldown::new());
            }
        }
    }

    // Einmal pro Tick: fremde Schild-Cooldowns herunterzaehlen. HEBT ein Spieler sein Schild
    // erneut (erst gesenkt, dann wieder oben), war die Schaetzung falsch (andere Waffe,
    // Plugin) -- dann ist der Eintrag weg. Nur "blockt gerade" reicht dafuer NICHT: der
    // Server schickt den Klang sofort, das Senken des Schilds (Metadaten) aber erst am
    // Tick-Ende. Frueher loeschte genau das den frischen Eintrag meist gleich wieder.
    pub(crate) fn tick_shield_cooldowns(&mut self) {
        let entities = &self.entities;
        self.shield_cooldowns.retain(|id, cd| {
            cd.rest = cd.rest.saturating_sub(1);
            let Some(e) = entities.get(*id) else { return false };
            let (rechts, links) = blocking_arms(e.using_item_right_arm(), e.arm_items());
            let blockt = rechts || links;
            if !blockt {
                cd.gesenkt = true;
            }
            let alter = SHIELD_DISABLE_TICKS - cd.rest;
            // Nie gesenkt, obwohl die Metadaten laengst da sein muessten: der Klang gehoerte
            // jemand anderem.
            let nie_gesenkt = !cd.gesenkt && alter > SHIELD_LOWER_GRACE_TICKS;
            cd.rest > 0 && !(cd.gesenkt && blockt) && !nie_gesenkt
        });
    }

    /// Ein Geraeusch vom Server abspielen (s. net::ClientEvent::Sound).
    pub(crate) fn play_server_sound(&mut self, sound: protocol::packets::SoundEffect) {
        self.play_sound_effect(sound, "server");
    }

    /// Wie play_server_sound, fuer Klaenge, die der Client aus einem anderen Paket ableitet
    /// oder selbst ausloest -- `herkunft` steht nur im Klang-Mitschnitt (s. sound_log).
    pub(crate) fn play_sound_effect(&mut self, sound: protocol::packets::SoundEffect, herkunft: &str) {
        if sound.name == "minecraft:item.shield.break" {
            self.note_shield_break(&sound);
        }
        // Consumable Optimizer: Ess- und Ruelpsklang haben wir zum richtigen Tick schon selbst
        // gespielt -- die verspaetete Fassung des Servers nicht noch einmal.
        let eigener = sound.entity_id.is_some_and(|id| Some(id) == self.own_entity_id)
            || sound.position.is_some_and(|(x, y, z)| DVec3::new(x, y, z).distance(self.physics.pos) < 2.0);
        if eigener && self.consumable.swallow_sound(&sound.name, Instant::now()) {
            let ort = sound.position.map(|(x, y, z)| DVec3::new(x, y, z));
            self.log_world_sound(herkunft, &sound, ort, "geschluckt: schon selbst gespielt (Consumable Optimizer)");
            return;
        }

        // Wo steht die Quelle? Bei einem ortsgebundenen Geraeusch steht es im Paket, bei einem
        // entity-gebundenen muss die Entity nachgeschlagen werden -- sie bewegt sich ja, der
        // Server kann ihre Position nicht mitschicken.
        let position = match (sound.position, sound.entity_id) {
            (Some((x, y, z)), _) => Some(DVec3::new(x, y, z)),
            // Uns selbst betreffende Geraeusche: die eigene Entity steht nicht in
            // `entities` (dort stehen nur die anderen), also die eigene Augenposition
            // nehmen -- sonst blieben Essen, Trinken und Rueststuecke stumm.
            (None, Some(id)) if Some(id) == self.own_entity_id => Some(self.physics.eye_pos()),
            (None, Some(id)) => self.entities.get(id).map(|e| DVec3::new(e.pos.0, e.pos.1, e.pos.2)),
            (None, None) => None,
        };
        // Entity noch nicht (mehr) bekannt -- kommt vor, wenn ein Geraeusch knapp vor dem
        // Spawn-Paket oder nach dem Entfernen eintrifft. Still bleiben ist richtig: an einer
        // geratenen Position waere die Ortung schlimmer als kein Ton.
        let Some(position) = position else {
            self.log_world_sound(herkunft, &sound, None, "verworfen: Entity nicht im Weltzustand");
            if net::sound_debug() {
                eprintln!(
                    "[ton] {} VERWORFEN: Entity {:?} nicht im Weltzustand (eigene Id {:?}, {} Entities bekannt)",
                    sound.name,
                    sound.entity_id,
                    self.own_entity_id,
                    self.entities.len()
                );
            }
            return;
        };

        // Pop-Klaenge (s. toteming.rs): Explosion und Anker-Laden austauschen, BEVOR die
        // Vanilla-Aufnahme geladen wird -- genau das, was ein Resourcepack mit "replace"
        // macht, nur umschaltbar (SoundReplacements der Mod).
        if self.settings.toteming.sounds {
            if let Some(slot) = toteming::SoundSlot::replacing(&sound.name) {
                if !self.settings.toteming.slot(slot).is_vanilla() {
                    self.log_world_sound(herkunft, &sound, Some(position), "ersetzt: Pop-Klaenge-Modul");
                    if let Some(v) = self.pop_voice(slot, sound.seed) {
                        let placement = self.listener().placement(position);
                        self.audio.sink().play_sample_at(v.sample, v.gain, v.pitch, &placement);
                    }
                    return;
                }
            }
        }

        let Some(aufnahme) = self.vanilla_sounds.resolve(&mut self.installation, &sound.name, sound.seed) else {
            self.log_world_sound(herkunft, &sound, Some(position), "verworfen: keine abspielbare Aufnahme");
            if net::sound_debug() {
                eprintln!("[ton] {} VERWORFEN: keine abspielbare Aufnahme", sound.name);
            }
            return;
        };

        // Die Faktoren aus sounds.json und aus dem Paket multiplizieren sich -- eine von Haus
        // aus leise abgemischte Variante bleibt auch dann leiser, wenn der Server volle
        // Lautstaerke verlangt.
        //
        // Lautstaerken ueber 1 sind in Vanilla KEIN lauteres Abspielen, sondern eine groessere
        // Reichweite. Hier bleibt der Pegel deshalb gedeckelt: sonst waere ein weit hoerbares
        // Ereignis (Wither, Drache) aus der Naehe ohrenbetaeubend.
        let gain = aufnahme.volume * sound.volume.min(1.0) * self.settings.sound_gain(sound.category);
        if gain <= 0.0 {
            self.log_world_sound(herkunft, &sound, Some(position), "stumm: Lautstaerke 0");
            if net::sound_debug() {
                eprintln!("[ton] {} VERWORFEN: Lautstärke 0 (Kategorie {:?})", sound.name, sound.category);
            }
            return;
        }
        if net::sound_debug() {
            let abstand = (position - self.physics.eye_pos()).length();
            eprintln!("[ton] {} GESPIELT: {abstand:.1} Blöcke weg, Pegel {gain:.3}, kat={:?}", sound.name, sound.category);
        }
        let pitch = aufnahme.pitch * sound.pitch.max(1e-3);
        if sound_log::enabled() {
            self.log_world_sound(herkunft, &sound, Some(position), &format!("gespielt  pegel={gain:.3} tonhoehe={pitch:.2}"));
        }

        // Jedes Server-Geraeusch steht im Raum, auch Musik und Schallplatten: Vanilla spielt
        // beide Pakete ueber `ClientLevel.playSeededSound` an der Paketposition (im
        // Client nachgesehen) -- kopffest ist nur die Hintergrundmusik, die der
        // Client selbst waehlt.
        //
        // Nachgefuehrt statt einmal platziert: die Lage wird jedes Bild neu gerechnet (s.
        // update_tracked_sounds), und ein entity-gebundenes Geraeusch wandert mit der Entity
        // (Vanilla `EntityBoundSoundInstance.tick`).
        let quelle = match sound.entity_id {
            Some(id) if Some(id) == self.own_entity_id => SoundSource::Own,
            Some(id) => SoundSource::Entity(id, position),
            None => SoundSource::Fixed(position),
        };
        let tag = audio::SoundTag { category: sound.category as u8, name: audio::sound_name_hash(&sound.name) };
        self.play_tracked(aufnahme.sample, gain, pitch, quelle, tag);
    }

    // Klang-Mitschnitt (s. sound_log): eine Zeile fuer ein Geraeusch in der Welt. Verursacher
    // und Abstand werden nur ausgerechnet, wenn der Mitschnitt laeuft.
    pub(crate) fn log_world_sound(&self, herkunft: &str, sound: &protocol::packets::SoundEffect, ort: Option<DVec3>, ergebnis: &str) {
        self.log_client_sound(herkunft, &sound.name, sound.category, sound.entity_id, ort, sound.volume, sound.pitch, ergebnis);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn log_client_sound(&self, herkunft: &str, name: &str, kategorie: protocol::packets::SoundCategory, entity_id: Option<i32>, ort: Option<DVec3>, volume: f32, pitch: f32, ergebnis: &str) {
        if !sound_log::enabled() {
            return;
        }
        let wer = self.sound_log_source(entity_id, ort);
        let abstand = ort.map(|p| (p - self.physics.eye_pos()).length());
        sound_log::sound(herkunft, name, kategorie, &wer, abstand, volume, pitch, ergebnis);
    }

    // Wer hat den Klang gemacht? Bei entity-gebundenen steht es im Paket. Ortsgebundene
    // (darunter alle Angriffsklaenge: Player.attack spielt an der Position des Angreifers)
    // bekommen die naechste Entity hoechstens 2 Bloecke daneben -- geraten, aber bei einem
    // Kampf fast immer richtig.
    pub(crate) fn sound_log_source(&self, entity_id: Option<i32>, ort: Option<DVec3>) -> String {
        let beschreibe = |id: i32| -> String {
            if Some(id) == self.own_entity_id {
                return "selbst".to_string();
            }
            match self.entities.get(id) {
                Some(e) if world::is_player(e.entity_type) => {
                    format!("Spieler {} #{id}", self.player_names.get(&e.uuid).map(String::as_str).unwrap_or("?"))
                }
                Some(e) => format!("{} #{id}", world::entity_type_name(e.entity_type).unwrap_or("?")),
                None => format!("unbekannt #{id}"),
            }
        };
        if let Some(id) = entity_id {
            return beschreibe(id);
        }
        let Some(p) = ort else { return "-".to_string() };
        let eigen = (p - self.physics.pos).length();
        let fremd = self
            .entities
            .iter()
            .map(|(id, e)| (*id, (p - DVec3::new(e.pos.0, e.pos.1, e.pos.2)).length()))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match fremd {
            Some((id, d)) if d < eigen && d <= 2.0 => format!("nahe {} ({d:.1})", beschreibe(id)),
            _ if eigen <= 2.0 => format!("nahe selbst ({eigen:.1})"),
            _ => "Ort".to_string(),
        }
    }

    /// Ein Weltgeraeusch nachgefuehrt abspielen (s. update_tracked_sounds). Rueckgabe: der Griff.
    pub(crate) fn play_tracked(&mut self, sample: Arc<audio::wav::Wav>, gain: f32, pitch: f32, quelle: SoundSource, tag: audio::SoundTag) -> Option<u32> {
        let dauer = sample.duration() / pitch.max(1e-3);
        let placement = self.listener().placement(self.sound_source_position(&quelle));
        let handle = self.audio.sink().play_sample_tracked(sample, gain, pitch, Some(&placement), tag)?;
        // Kurze Klaenge brauchen keine Nachfuehrung -- in den paar Hundert Millisekunden
        // bewegt sich nichts hoerbar, und die Liste bleibt klein (Schritte kommen zu Dutzenden).
        if dauer >= TRACK_MIN_SECONDS {
            self.tracked_sounds.push(TrackedSound { handle, source: quelle, ends: Instant::now() + Duration::from_secs_f32(dauer + 0.1) });
        }
        Some(handle)
    }

    // Wo steht eine Geraeuschquelle JETZT? Bei Entities die gezeichnete (interpolierte)
    // Position; ist die Entity verschwunden, bleibt die letzte bekannte.
    pub(crate) fn sound_source_position(&self, quelle: &SoundSource) -> DVec3 {
        match quelle {
            SoundSource::Fixed(p) => *p,
            SoundSource::Own => self.physics.eye_pos(),
            SoundSource::Entity(id, zuletzt) => {
                self.entities.get(*id).map(|e| DVec3::from(e.render_pos(self.tick_progress()))).unwrap_or(*zuletzt)
            }
        }
    }

    /// Laufende Weltgeraeusche an Kopf und Quelle anpassen -- einmal pro Bild.
    pub(crate) fn update_tracked_sounds(&mut self) {
        if self.tracked_sounds.is_empty() {
            return;
        }
        let jetzt = Instant::now();
        self.tracked_sounds.retain(|t| t.ends > jetzt);
        let listener = self.listener();
        let progress = self.tick_progress();
        let mut lagen = Vec::with_capacity(self.tracked_sounds.len());
        for t in self.tracked_sounds.iter_mut() {
            if let SoundSource::Entity(id, zuletzt) = &mut t.source {
                if let Some(e) = self.entities.get(*id) {
                    *zuletzt = DVec3::from(e.render_pos(progress));
                }
            }
            let position = match &t.source {
                SoundSource::Fixed(p) | SoundSource::Entity(_, p) => *p,
                SoundSource::Own => self.physics.eye_pos(),
            };
            lagen.push((t.handle, listener.placement(position)));
        }
        self.audio.sink().move_voices(&lagen);
    }

    /// Stop-Sound-Paket: laufende Weltgeraeusche anhalten (Vanilla `SoundManager.stop`).
    pub(crate) fn stop_server_sounds(&mut self, category: Option<u8>, name: Option<&str>) {
        let name = name.map(audio::sound_name_hash);
        self.audio.sink().stop_sounds(category, name);
        if category.is_none() && name.is_none() {
            self.jukeboxes.clear();
        }
    }

    /// Level Event 1010: eine Jukebox beginnt ein Lied (Vanilla `LevelEventHandler.playJukeboxSong`
    /// -- RECORDS, Lautstaerke 4, also 64 Bloecke Reichweite, an der Blockmitte).
    pub(crate) fn start_jukebox(&mut self, pos: (i32, i32, i32), song: i32) {
        self.stop_jukebox(pos);
        let Some(name) = usize::try_from(song).ok().and_then(|i| self.jukebox_songs.get(i)).cloned() else { return };
        let Some(aufnahme) = self.vanilla_sounds.resolve(&mut self.installation, &name, 0) else { return };
        let gain = aufnahme.volume * self.settings.sound_gain(protocol::packets::SoundCategory::Records);
        if gain <= 0.0 {
            return;
        }
        let mitte = DVec3::new(pos.0 as f64 + 0.5, pos.1 as f64 + 0.5, pos.2 as f64 + 0.5);
        self.log_client_sound("levelevent", &name, protocol::packets::SoundCategory::Records, None, Some(mitte), 4.0, 1.0, &format!("gespielt  pegel={gain:.3} (Jukebox)"));
        let tag = audio::SoundTag { category: protocol::packets::SoundCategory::Records as u8, name: audio::sound_name_hash(&name) };
        if let Some(handle) = self.play_tracked(aufnahme.sample, gain, aufnahme.pitch, SoundSource::Fixed(mitte), tag) {
            self.jukeboxes.insert(pos, handle);
        }
    }

    /// Level Event 1011: die Jukebox an `pos` verstummt.
    pub(crate) fn stop_jukebox(&mut self, pos: (i32, i32, i32)) {
        if let Some(handle) = self.jukeboxes.remove(&pos) {
            self.audio.sink().stop_handle(handle);
        }
    }

    /// Der eigene Schadenston -- den muss der Client selbst spielen.
    ///
    /// Warum: `PlayerEntity.playSound` uebergibt an `World.playSound` den Spieler SELBST als
    /// auszunehmenden Empfaenger (im Jar nachgesehen: `aload_0`, nicht `aconst_null` wie bei
    /// gewoehnlichen Entities). Auf dem Server heisst das "an alle ausser diesem Spieler" --
    /// wir bekommen also kein Ton-Paket, wenn wir selbst getroffen werden. Vanilla spielt ihn
    /// stattdessen client-seitig in `LivingEntity.onDamaged`, ausgeloest vom Schadenspaket.
    ///
    /// Fremde Treffer kommen weiterhin als Paket: dort ist der ausgenommene Empfaenger der
    /// GETROFFENE, nicht wir.
    ///
    /// Tonhoehe wie in Vanilla: `1.0 + (zufall - zufall) * 0.2`. Zwei Zufallszahlen und nicht
    /// eine -- ihre Differenz haeuft sich um die Mitte, statt gleichmaessig zu streuen, und
    /// klingt dadurch weniger nach einem Effekt und mehr nach Stimme.
    pub(crate) fn play_own_hurt_sound(&mut self, damage_type: i32) {
        let effect = usize::try_from(damage_type).ok().and_then(|i| self.damage_type_effects.get(i).copied()).unwrap_or_default();
        let Some(aufnahme) = self.vanilla_sounds.resolve(&mut self.installation, effect.player_hurt_sound(), 0) else {
            return;
        };
        let gain = aufnahme.volume * self.settings.sound_gain(protocol::packets::SoundCategory::Players);
        if gain <= 0.0 {
            return;
        }
        self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
        let a = (zufallswert(self.type_pitch_seed) + 1.0) * 0.5;
        self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
        let b = (zufallswert(self.type_pitch_seed) + 1.0) * 0.5;
        let pitch = aufnahme.pitch * (1.0 + (a - b) * 0.2);
        self.log_client_sound("client", effect.player_hurt_sound(), protocol::packets::SoundCategory::Players, self.own_entity_id, None, 1.0, pitch, &format!("gespielt  pegel={gain:.3} (eigener Schaden)"));
        // OHNE Ortung: der Klang sitzt im eigenen Kopf, er hat keine Richtung. Durch das
        // Ohrenmodell geschickt waere er zwar rechnerisch mittig (Abstand 0), aber die Absicht
        // waere nicht mehr abzulesen.
        self.audio.sink().play_sample(aufnahme.sample, gain, 0.0, pitch);
    }

    // PipeWire messen, solange der Audio-Tab offen ist; nach einem Neustart von pipewire-pulse
    // den eigenen Strom neu oeffnen (der alte ist mit dem Dienst gestorben).
    pub(crate) fn pipewire_tick(&mut self) {
        if self.pipewire.strom_neu_oeffnen() {
            if let Err(e) = self.audio.set_buffer_frames(audio_buffer_request(self.settings.audio_buffer_frames)) {
                self.push_log(format!("Audio nach PipeWire-Neustart nicht wieder geöffnet: {e}"));
            }
        }
        if self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsAudio {
            self.pipewire.messen_wenn_faellig();
        }
    }

    // Zeile "PipeWire aktiv": die gemessene Blockgroesse des Treibers.
    pub(crate) fn pipewire_text(&self) -> String {
        match self.pipewire.messung() {
            Some(m) => format!("{} Frames · {} ms", m.quantum, format!("{:.1}", m.ms()).replace('.', ",")),
            None if self.pipewire.beschaeftigt() => "misst".to_string(),
            None => "kein PipeWire gefunden".to_string(),
        }
    }

    // Was die Zeile "PipeWire-Grenze" zeigt. Ziel ist der eingestellte Puffer, solange er unter
    // der Vorgabe von pipewire-pulse liegt, auf eine Zweierpotenz aufgerundet (s.
    // pipewire::grenze_fuer).
    pub(crate) fn pipewire_knopf(&self) -> PipewireKnopf {
        let Some(m) = self.pipewire.messung() else { return PipewireKnopf::Text(self.pipewire_text()) };
        if let Some(f) = self.pipewire.fehler() {
            return PipewireKnopf::Knopf(format!("Fehlgeschlagen: {f}"), m.eigene_grenze);
        }
        let puffer = self.settings.audio_buffer_frames;
        let ziel = (puffer != 0 && puffer < PIPEWIRE_PULSE_MIN).then(|| pipewire::grenze_fuer(puffer, m.min_quantum));
        match (m.eigene_grenze, ziel) {
            (Some(e), z) if z.is_none() || z == Some(e) => PipewireKnopf::Knopf(format!("{e} Frames gesetzt · entfernen"), None),
            (_, Some(z)) => PipewireKnopf::Knopf(format!("Auf {z} Frames senken"), Some(z)),
            _ => PipewireKnopf::Text("Vorgabe reicht für diesen Puffer".to_string()),
        }
    }

    // Den Raumklang-Hoertest um einen Frame weiterdrehen (s. spatial_sound::OrbitTest).
    pub(crate) fn advance_orbit_test(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_orbit_update).as_secs_f32();
        self.last_orbit_update = now;
        let Some(mut test) = self.orbit_test.take() else { return };
        // Der Kopf steht fuer die ganze Runde fest, auch wenn sich der Spieler dabei dreht:
        // sonst waere die Runde keine Runde mehr, sondern eine Mischung aus Quellbewegung und
        // Kopfdrehung -- und genau die beiden auseinanderzuhalten ist ja der Zweck.
        let listener = self.listener();
        if let Some(position) = test.advance(dt, &listener) {
            // Ein trockener Klick: breitbandig und kurz. Beides ist noetig, damit die Ortung
            // ueberhaupt zu hoeren ist -- ein langer, schmalbandiger Ton hat keine klare
            // Anfangsflanke, und ohne die hat das Gehoer nichts, woran es die Laufzeit
            // zwischen den Ohren messen koennte.
            self.play_world_sound(ORBIT_TEST_SOUND, position, 0.8, 1.0);
        }
        if !test.finished() {
            self.orbit_test = Some(test);
        }
    }

    // Der Erwaehnungston, hoechstens alle 300 ms -- ein Spam darf nicht zum Dauerton werden.
    pub(crate) fn play_ego_sound(&mut self) {
        if self.settings.ego_sound.is_empty() || self.ego_sound_at.is_some_and(|t| t.elapsed().as_millis() < 300) {
            return;
        }
        self.ego_sound_at = Some(Instant::now());
        self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
        let seed = (zufallswert(self.type_pitch_seed) * i64::MAX as f32) as i64;
        let event = self.settings.ego_sound.clone();
        let Some(aufnahme) = self.vanilla_sounds.resolve(&mut self.installation, &event, seed) else { return };
        // Ohne die Lautstaerke aus sounds.json: manche Klaenge stehen dort sehr leise (Amethyst
        // 0.2), hier regelt allein der eigene Regler.
        let gain = self.settings.ego_volume / 100.0 * self.settings.sound_gain(protocol::packets::SoundCategory::Players);
        if gain > 0.0 {
            self.audio.sink().play_sample(aufnahme.sample, gain, 0.0, aufnahme.pitch);
        }
    }

    // Ein Klang am eigenen Kopf, den der Server uns nicht schickt (World.playSound mit uns als
    // Ausnahme -- s. play_own_hurt_sound).
    // Wie play_own_event, nur an einem Ort in der Welt (z.B. der Block, in den man ausleert).
    pub(crate) fn play_event_at(&mut self, event: &str, ort: DVec3, category: protocol::packets::SoundCategory) {
        self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
        let seed = (zufallswert(self.type_pitch_seed) * i64::MAX as f32) as i64;
        let Some(aufnahme) = self.vanilla_sounds.resolve(&mut self.installation, event, seed) else {
            self.log_client_sound("client", event, category, None, Some(ort), 1.0, 1.0, "verworfen: keine abspielbare Aufnahme");
            return;
        };
        let gain = aufnahme.volume * self.settings.sound_gain(category);
        if gain <= 0.0 {
            self.log_client_sound("client", event, category, None, Some(ort), 1.0, 1.0, "stumm: Lautstaerke 0");
            return;
        }
        self.log_client_sound("client", event, category, None, Some(ort), 1.0, 1.0, &format!("gespielt  pegel={gain:.3}"));
        let placement = self.listener().placement(ort);
        self.audio.sink().play_sample_at(aufnahme.sample, gain, aufnahme.pitch, &placement);
    }

    // Wie play_event_at, mit eigener Lautstaerke und Tonhoehe (Entity.playSound(event, volume,
    // pitch)) -- beides wird mit dem multipliziert, was sounds.json fuer die Aufnahme angibt.
    pub(crate) fn play_event_at_scaled(&mut self, event: &str, ort: DVec3, volume: f32, pitch: f32, category: protocol::packets::SoundCategory) {
        self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
        let seed = (zufallswert(self.type_pitch_seed) * i64::MAX as f32) as i64;
        let Some(aufnahme) = self.vanilla_sounds.resolve(&mut self.installation, event, seed) else {
            self.log_client_sound("client", event, category, None, Some(ort), volume, pitch, "verworfen: keine abspielbare Aufnahme");
            return;
        };
        let gain = aufnahme.volume * volume * self.settings.sound_gain(category);
        if gain <= 0.0 {
            self.log_client_sound("client", event, category, None, Some(ort), volume, pitch, "stumm: Lautstaerke 0");
            return;
        }
        self.log_client_sound("client", event, category, None, Some(ort), volume, pitch, &format!("gespielt  pegel={gain:.3}"));
        let placement = self.listener().placement(ort);
        self.audio.sink().play_sample_at(aufnahme.sample, gain, aufnahme.pitch * pitch, &placement);
    }

    // Entity.applyMovementEmissionAndPlaySound fuer den eigenen Spieler. Die Schritte spielt in
    // Vanilla der eigene Client -- der Server schickt sie nur den ANDEREN (Player.playSound mit
    // sich selbst als Ausnahme). Deshalb fehlten sie hier frueher ganz.
    //
    // Je Tick waechst moveDist um die gelaufene Strecke mal 0.6 (beim Klettern auch die
    // senkrechte), und jedes Mal, wenn es die naechste ganze Zahl ueberschreitet, klingt ein
    // Schritt. Player.getMovementEmission: schleichend am Boden gar nichts.
    pub(crate) fn tick_step_sounds(&mut self, vorher: DVec3) {
        if self.is_dead || self.game_mode == GAME_MODE_SPECTATOR || self.own_ride.is_some() {
            return;
        }
        if self.physics.on_ground && self.physics.crouching {
            return;
        }
        let d = self.physics.pos - vorher;
        let gesamt = (d.length() * 0.6) as f32;
        let waagerecht = ((d.x * d.x + d.z * d.z).sqrt() * 0.6) as f32;
        let p = self.physics.pos;
        let (bx, bz) = (p.x.floor() as i32, p.z.floor() as i32);
        // getOnPos (1e-5 unter den Fuessen) und getOnPosLegacy (0.2 darunter, der Block, der
        // klingt). Vanillas mainSupportingBlockPos (Kante eines Blocks) ist hier die Mitte.
        let stuetz_pos = (bx, (p.y - 1.0e-5).floor() as i32, bz);
        let effekt_pos = (bx, (p.y - 0.2).floor() as i32, bz);
        let stuetz = self.world.block_at(stuetz_pos.0, stuetz_pos.1, stuetz_pos.2);
        self.step_move_dist += if step_climbable(stuetz) { gesamt } else { waagerecht };
        if self.step_move_dist <= self.step_next || stuetz == world::STATE_AIR {
            return;
        }
        let effekt = self.world.block_at(effekt_pos.0, effekt_pos.1, effekt_pos.2);
        let nur_effekt = stuetz_pos == effekt_pos;
        let mut erzeugt = self.step_side_effects(effekt_pos, effekt, true, d);
        if !nur_effekt {
            erzeugt |= self.step_side_effects(stuetz_pos, stuetz, false, d);
        }
        if erzeugt {
            self.step_next = self.step_move_dist.trunc() + 1.0;
        } else if self.physics.touching_water {
            self.step_next = self.step_move_dist.trunc() + 1.0;
            self.water_swim_sound();
        }
    }

    // Entity.vibrationAndSoundEffectsFromBlock: klingt nur am Boden, beim Klettern oder
    // schleichend ohne Hoehenaenderung -- und nie beim Schwimmen.
    pub(crate) fn step_side_effects(&mut self, pos: (i32, i32, i32), state: u32, klang: bool, d: DVec3) -> bool {
        if state == world::STATE_AIR {
            return false;
        }
        let ph = &self.physics;
        if (ph.on_ground || step_climbable(state) || ph.crouching && d.y == 0.0) && !ph.swimming {
            if klang {
                self.player_step_sound(pos, state);
            }
            true
        } else {
            false
        }
    }

    // Player.playStepSound: im Wasser Schwimmgeraeusch plus gedaempfter Schritt; sonst klingt
    // ein Teppich, Schnee o.ae. darueber mit (COMBINATION_STEP_SOUND_BLOCKS) bzw. statt des
    // Blocks (INSIDE_STEP_SOUND_BLOCKS).
    pub(crate) fn player_step_sound(&mut self, pos: (i32, i32, i32), state: u32) {
        if self.physics.touching_water {
            self.water_swim_sound();
            self.play_step_group(state, 0.05, 0.8);
            return;
        }
        let oben = self.world.block_at(pos.0, pos.1 + 1, pos.2);
        let oben_name = world::block_name(oben).unwrap_or("");
        if world::block_sounds::is_combination_step_sound(oben_name) {
            self.play_step_group(oben, 0.15, 1.0);
            self.play_step_group(state, 0.05, 0.8);
        } else if world::block_sounds::is_inside_step_sound(oben_name) {
            self.play_step_group(oben, 0.15, 1.0);
        } else {
            self.play_step_group(state, 0.15, 1.0);
        }
    }

    // Der Schritt-Klang eines Blocks: Lautstaerke und Tonhoehe seiner Klanggruppe mal Faktor,
    // an den eigenen Fuessen (Entity.playSound spielt an getX/getY/getZ).
    // Ein Blockklang an der Blockmitte (playSoundAtBlockCenter / PositionedSoundInstance mit
    // BlockPos).
    pub(crate) fn play_block_sound(&mut self, event: &str, pos: (i32, i32, i32), volume: f32, pitch: f32) {
        let mitte = DVec3::new(pos.0 as f64 + 0.5, pos.1 as f64 + 0.5, pos.2 as f64 + 0.5);
        self.play_event_at_scaled(event, mitte, volume, pitch, protocol::packets::SoundCategory::Blocks);
    }

    // LevelEventHandler, Event 2001: Abbauklang des alten Blocks, (v+1)/2 laut, Tonhoehe p*0.8.
    pub(crate) fn play_block_break_sound(&mut self, pos: (i32, i32, i32), state: u32) {
        if state == world::STATE_AIR {
            return;
        }
        let gruppe = world::block_sounds::sound_group(world::block_name(state).unwrap_or(""));
        self.play_block_sound(gruppe.break_sound, pos, (gruppe.volume + 1.0) / 2.0, gruppe.pitch * 0.8);
    }

    // BlockItem.place: world.playSound(player, ...) -- der Server schickt den Klang allen
    // AUSSER dem Setzenden, der hoert ihn nur aus dem eigenen Client. Die Gruppe haengt am
    // Block, nicht am Zustand, deshalb reicht der Itemname (Blockitems heissen wie ihr Block).
    pub(crate) fn play_place_sound(&mut self, pos: (i32, i32, i32), block: &str) {
        let gruppe = world::block_sounds::sound_group(block);
        self.play_block_sound(gruppe.place, pos, (gruppe.volume + 1.0) / 2.0, gruppe.pitch * 0.8);
    }

    pub(crate) fn play_step_group(&mut self, state: u32, lautstaerke: f32, tonhoehe: f32) {
        let gruppe = world::block_sounds::sound_group(world::block_name(state).unwrap_or(""));
        if gruppe.step.is_empty() {
            return;
        }
        self.play_event_at_scaled(gruppe.step, self.physics.pos, gruppe.volume * lautstaerke, gruppe.pitch * tonhoehe, protocol::packets::SoundCategory::Players);
    }

    // Entity.waterSwimSound + playSwimSound: leiser bei langsamer Bewegung, Tonhoehe gestreut.
    pub(crate) fn water_swim_sound(&mut self) {
        let v = self.physics.vel;
        let tempo = (((v.x * v.x * 0.2 + v.y * v.y + v.z * v.z * 0.2).sqrt()) as f32 * 0.35).min(1.0);
        self.type_pitch_seed = self.type_pitch_seed.wrapping_add(2);
        // random.nextFloat() - random.nextFloat(); zufallswert liefert -1..1, also halbiert.
        let streuung = (zufallswert(self.type_pitch_seed) - zufallswert(self.type_pitch_seed.wrapping_add(1))) / 2.0;
        self.play_event_at_scaled("entity.player.swim", self.physics.pos, tempo, 1.0 + streuung * 0.4, protocol::packets::SoundCategory::Players);
    }

    // LivingEntity.onKineticHit: der "Pieks", wenn ein Speer-Ansturm trifft. Der Server meldet
    // den Treffer nur als Entity-Ereignis am ANGREIFER, den Klang spielt jeder Client selbst
    // -- fehlte frueher ganz. Klang des benutzten Items, bei uns nach dem Speer: Holz oder
    // die anderen.
    pub(crate) fn kinetic_hit_sound(&mut self, entity_id: i32) {
        let jetzt = Instant::now();
        if self.kinetic_hit_at.get(&entity_id).is_some_and(|t| jetzt.duration_since(*t) <= KINETIC_HIT_FEEDBACK) {
            return;
        }
        self.kinetic_hit_at.retain(|_, t| jetzt.duration_since(*t) <= KINETIC_HIT_FEEDBACK);
        let eigener = self.own_entity_id == Some(entity_id);
        let item = if eigener {
            let hand = if self.using_item && self.using_offhand { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
            self.own_hand_stack(hand).filter(|st| world::item_behavior::kinetic(st).is_some()).and_then(|st| world::item_registry::item_name(st.item_id))
        } else {
            self.entities.get(entity_id).and_then(|e| e.main_hand.as_ref()).and_then(|h| world::item_registry::item_name(h.item_id)).filter(|n| n.ends_with("_spear"))
        };
        let Some(item) = item else { return };
        self.kinetic_hit_at.insert(entity_id, jetzt);
        let klang = if item == "wooden_spear" { "item.spear_wood.hit" } else { "item.spear.hit" };
        if eigener {
            self.play_own_event(klang);
        } else if let Some(e) = self.entities.get(entity_id) {
            let (x, y, z) = e.pos;
            // Speere tragen ausser Spielern nur Monster.
            let kategorie = if world::is_player(e.entity_type) { protocol::packets::SoundCategory::Players } else { protocol::packets::SoundCategory::Hostile };
            self.play_event_at(klang, DVec3::new(x, y, z), kategorie);
        }
    }

    // Kategorie Spieler: es ist der eigene Spieler, der den Klang macht.
    pub(crate) fn play_own_event(&mut self, event: &str) {
        self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
        let seed = (zufallswert(self.type_pitch_seed) * i64::MAX as f32) as i64;
        let Some(aufnahme) = self.vanilla_sounds.resolve(&mut self.installation, event, seed) else {
            self.log_client_sound("client", event, protocol::packets::SoundCategory::Players, self.own_entity_id, None, 1.0, 1.0, "verworfen: keine abspielbare Aufnahme");
            return;
        };
        let gain = aufnahme.volume * self.settings.sound_gain(protocol::packets::SoundCategory::Players);
        if gain <= 0.0 {
            self.log_client_sound("client", event, protocol::packets::SoundCategory::Players, self.own_entity_id, None, 1.0, 1.0, "stumm: Lautstaerke 0");
            return;
        }
        self.log_client_sound("client", event, protocol::packets::SoundCategory::Players, self.own_entity_id, None, 1.0, 1.0, &format!("gespielt  pegel={gain:.3}"));
        let placement = self.listener().placement(self.physics.eye_pos());
        self.audio.sink().play_sample_at(aufnahme.sample, gain, aufnahme.pitch, &placement);
    }
}
