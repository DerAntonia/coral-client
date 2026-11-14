// Eingabefelder, Konfigurationen wechseln und speichern, Lichtseite, Schriften.

use super::*;

impl App {
    // Laufendes Ziehen am Regler (s. slider_drag): bei jeder Zeigerbewegung.
    pub(crate) fn slider_drag_update(&mut self) {
        if let Some(ev) = self.slider_drag {
            self.slider_set_from_pointer(ev);
        }
    }

    // Zeichnet ein Zahlen-Wertfeld: waehrend es bearbeitet wird (editing_value) den
    // Live-Puffer linksbuendig mit blinkendem Caret (wie das Adressfeld auf dem
    // Connect-Screen), sonst den normal formatierten Wert zentriert.
    pub(crate) fn draw_editable_value(
        &self,
        gpu: &GpuState,
        rect: Rect,
        ev: EditableValue,
        mut formatted: String,
        rects: &mut Vec<UiRect>,
        texts: &mut Vec<UiText>,
    ) {
        // Als Regler (Design-System): Bahn mit gefuelltem Teil, Daumen, rechts der Wert. Das
        // Wertfeld selbst ist dann nur die Anzeige rechts (dort wird getippt).
        let rect = if let Some(r) = ev.slider() {
            let bahn = ev.slider_track();
            let max = self.slider_max(ev);
            let t = self.glide_slider(SliderKey::Value(ev), ((self.editable_number(ev) - r.min) / (max - r.min).max(1e-9)) as f32);
            let aktiv = self.slider_drag == Some(ev);
            let (qx, qy) = self.pointer();
            Self::push_slider_visual(rects, bahn, t, aktiv || ev.slider_rect().contains(qx, qy), aktiv, true);
            // Mit gehaltener rechter Taste: das rechte Ende klein unter der Bahn.
            if aktiv && self.slider_extended {
                self.push_slider_max_label(gpu, bahn, format!("{max:.*}", r.decimals), texts);
            }
            let (kasten, zahl) = push_slider_unit(gpu, ev.slider_value_rect(), &formatted, texts);
            formatted = zahl;
            kasten
        } else {
            rect
        };
        rects.push(UiRect::framed(rect, theme::get(theme::Slot::SurfaceSunken), 6.0, 1.0, theme::get(theme::Slot::Border1)));
        if let Some((editing_ev, buf)) = &self.editing_value {
            if *editing_ev == ev {
                self.push_value_input(gpu, rect, formatted, buf, rects, texts);
                return;
            }
        }
        texts.push(gpu.centered_text(rect, formatted, theme::text(), UI_SCALE_SMALL));
    }

    // Das offene Eingabefeld uebernehmen (OK-Knopf oder Enter).
    pub(crate) fn submit_input_field(&mut self) {
        let Some((kind, text)) = self.input_field.take() else { return };
        match kind {
            InputKind::Token => self.submit_token_login(text, accounts::Source::Token),
            InputKind::RefreshToken => self.submit_refresh_token_login(text),
            InputKind::ConfigName(edit) => {
                // Vorher speichern: Duplizieren kopiert die Datei, und die soll den Stand von
                // jetzt tragen.
                self.persist_settings();
                let aktiv = active_config();
                let ergebnis = match edit {
                    ConfigEdit::New => assets::create_config(&text, None),
                    ConfigEdit::Duplicate => assets::create_config(&text, Some(&aktiv)),
                    ConfigEdit::Rename => assets::rename_config(&aktiv, &text),
                };
                match ergebnis {
                    Ok(name) if edit == ConfigEdit::Rename => {
                        set_active_config(&name);
                        assets::set_last_used(&name);
                    }
                    Ok(name) => self.switch_config(&name),
                    Err(e) => {
                        // Feld mit dem Getippten wieder auf, damit man nur korrigieren muss.
                        self.input_field = Some((kind, text));
                        self.notify(notifications::Notification::error("Config", e.to_string()));
                    }
                }
            }
        }
    }

    // Im laufenden Spiel zu einer anderen Config wechseln: die aktuelle speichern, die andere
    // laden und alles nachziehen, was sonst nur beim Start oder beim Klick auf die jeweilige
    // Einstellung passiert.
    pub(crate) fn switch_config(&mut self, name: &str) {
        if name == active_config() {
            return;
        }
        self.persist_settings();
        self.switch_config_unsaved(name);
    }

    pub(crate) fn switch_config_unsaved(&mut self, name: &str) {
        set_active_config(name);
        assets::set_last_used(name);
        let alt = self.adopt_settings(Settings::load());
        self.settings_scroll = Default::default();
        self.page_scrolls.clear();
        self.apply_switched_settings(&alt);
        self.persist_settings();
        self.notify(notifications::Notification::success("Config gewechselt", format!("Aktiv: {name}")));
    }

    // Vor dem Verbinden: gibt es eine Config, die so heisst wie der Server, wird sie aktiv. Die
    // vorher aktive merkt sich server_config_return fuer das Trennen. Ohne eigene Config gilt
    // wieder die von vor dem letzten Server (falls man ohne Trennen weiterverbindet).
    pub(crate) fn apply_server_config(&mut self) {
        let adresse = self.server_address.trim();
        let name = self.servers.iter().find(|s| s.address.trim().eq_ignore_ascii_case(adresse)).map(|s| s.name.as_str());
        let Some(ziel) = server_config_for(&assets::list_configs(), name, adresse) else {
            self.restore_config_after_server();
            return;
        };
        let vorher = match self.server_config_return.take() {
            Some((vorher, server)) if server == active_config() => vorher,
            _ => active_config(),
        };
        if ziel != vorher {
            self.server_config_return = Some((vorher, ziel.clone()));
        }
        self.switch_config(&ziel);
    }

    // Nach dem Trennen: die Config von vor dem Verbinden wieder aktiv -- nicht, wenn man
    // inzwischen selbst gewechselt hat oder es sie nicht mehr gibt.
    pub(crate) fn restore_config_after_server(&mut self) {
        let Some((vorher, server)) = self.server_config_return.take() else { return };
        if server == active_config() && assets::list_configs().contains(&vorher) {
            self.switch_config(&vorher);
        }
    }

    // Eigenes Schreiben der Config merken, damit poll_config_file es nicht fuer eine Aenderung
    // von aussen haelt.
    pub(crate) fn note_config_written(&mut self) {
        self.config_seen = Some(config_stamp());
    }

    // Wurde die Config-Datei von aussen geaendert (Editor, Skript), gilt sie sofort -- mit
    // denselben Nebeneffekten wie ein Config-Wechsel, aber ohne Scrollstaende zurueckzusetzen,
    // damit man beim Schrauben im Menue bleibt, wo man ist.
    pub(crate) fn poll_config_file(&mut self) {
        if self.config_checked.elapsed() < CONFIG_POLL_INTERVAL {
            return;
        }
        self.config_checked = Instant::now();
        let jetzt = config_stamp();
        let Some(gesehen) = self.config_seen.replace(jetzt.clone()) else {
            return;
        };
        if gesehen == jetzt || gesehen.path != jetzt.path {
            return;
        }
        let alt = self.adopt_settings(Settings::load());
        // Settings::load kann selbst schreiben (private Werte aus der Config abtrennen).
        self.note_config_written();
        self.apply_switched_settings(&alt);
        self.push_log(format!("Config \"{}\" von aussen geändert, neu geladen", active_config()));
    }

    // Setzt `neu` als laufende Einstellungen und gibt die alten zurueck.
    pub(crate) fn adopt_settings(&mut self, mut neu: Settings) -> Settings {
        let alt = std::mem::take(&mut self.settings);
        // Eine Config ohne Anmeldung uebernimmt die laufende: man bleibt eingeloggt, statt
        // beim naechsten Verbinden ploetzlich ohne Konto dazustehen.
        if neu.mc_access_token.is_empty() {
            neu.account_name = alt.account_name.clone();
            neu.account_uuid = alt.account_uuid.clone();
            neu.mc_access_token = alt.mc_access_token.clone();
            neu.msa_refresh_token = alt.msa_refresh_token.clone();
        } else {
            self.credentials = Some(auth::Credentials {
                access_token: neu.mc_access_token.clone(),
                profile_id: neu.account_uuid.clone(),
                username: neu.account_name.clone(),
            });
        }
        self.settings = neu;
        alt
    }

    // Was eine geaenderte Einstellung ausserhalb von self.settings nach sich zieht -- fuer
    // den Config-Wechsel, bei dem sich alles auf einmal aendern kann.
    pub(crate) fn apply_switched_settings(&mut self, alt: &Settings) {
        self.apply_frame_latency();
        self.mesh_pool.resize(self.settings.chunk_update_threads);
        if self.settings.audio_output_device != alt.audio_output_device {
            let geraet = (!self.settings.audio_output_device.is_empty()).then_some(self.settings.audio_output_device.as_str());
            if let Err(e) = self.audio.set_output_device(geraet) {
                eprintln!("[audio] Ausgabegerät der Config nicht verfügbar: {e}");
                let _ = self.audio.set_output_device(None);
            }
        }
        if self.settings.font != alt.font {
            self.hud_latched_widths.borrow_mut().clear();
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.stable_numbers.borrow_mut().clear();
                if let Err(e) = gpu.font_atlas.switch(&gpu.device, &gpu.queue, &mut self.installation, FontKind::from_index(self.settings.font)) {
                    eprintln!("Schrift konnte nicht gewechselt werden: {e}");
                }
            }
        }
        self.sync_text_shadow();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.stable_numbers.borrow_mut().set_enabled(self.settings.stable_numbers);
        }
        crate::font_atlas::set_capitals(self.settings.font_capitals);
        apply_language_settings(&mut self.installation, &self.settings);
        self.input_log.set_enabled(self.settings.module_input_log_enabled);
        record::set_enabled(self.settings.module_coral_record);
        record::set_clip_enabled(self.settings.module_coral_clip);
        self.publish_totem_sound_enabled();
        // Block-Rotation, weiche Beleuchtung, Markierungen: alles steckt im Mesh.
        self.block_uvs.set_rng_rotation_enabled(self.settings.block_rng_rotation);
        self.world.light_remesh = crate::shader_path_on();
        world::mesher::set_flat_face_shade(crate::shader_path_on());
        for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
            self.world.mark_dirty(cx, cz);
        }
        self.menu_world_frozen = false;
        if self.settings.active_skins != alt.active_skins || self.skin_exclusions_of(&self.settings.active_skins) != self.loaded_skin_exclusions {
            self.selected_skins = self.settings.active_skins.clone();
            self.reload_resource_pack(self.settings.active_skins.clone());
        }
    }

    pub(crate) fn particle_kinds(&self) -> Vec<(usize, &'static particles::ParticleKind)> {
        particles::particle_kinds_for_own(self.particle_screen_own)
    }


    // Nach jeder Aenderung sofort schreiben. Das sind ein paar Dutzend Byte bei einem
    // Klick -- billiger als jede Loesung, die sich merken muesste, wann etwas "offen" ist.
    // Den allgemeinen Rand samt Staerke an die GPU geben (s. GpuState::text_shadow_style).
    pub(crate) fn sync_text_shadow(&mut self) {
        let stil = self.settings.global_text_shadow();
        let staerke = self.settings.shadow_strength(0, stil);
        let halb = self.settings.shadow_half[0];
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.text_shadow_style = stil;
            gpu.text_shadow_strength = staerke;
            gpu.text_shadow_half = halb;
        }
    }

    pub(crate) fn persist_settings(&mut self) {
        // Waehrend ein Regler Schritte ausfuehrt, nicht bei jedem Schritt schreiben.
        if self.persist_paused {
            return;
        }
        // Scrollstaende mitschreiben -- sie aendern sich ohne eigenes Speichern (Mausrad).
        if self.scrolls_loaded {
            let staende: Vec<(&str, usize)> = self.scroll_slots().into_iter().map(|(n, w)| (n, w.ziel.max(0.0).round() as usize)).collect();
            for (name, wert) in staende {
                self.settings.settings_scrolls.insert(name.to_string(), wert);
            }
        }
        // Gespeichert wird still; nur ein Fehlschlag meldet sich (einmal, nicht bei jedem
        // weiteren Versuch mit demselben Fehler).
        let status = match self.settings.save() {
            Ok(_) => SaveStatus::Saved,
            Err(e) => SaveStatus::Failed(e.to_string()),
        };
        self.note_config_written();
        if let SaveStatus::Failed(e) = &status {
            if !matches!(&self.settings_status, Some(SaveStatus::Failed(alt)) if alt == e) {
                self.notify(notifications::Notification::error("Speichern fehlgeschlagen", e.clone()));
            }
        }
        self.settings_status = Some(status);
    }

    // Traegt das gespeicherte Render-Ahead-Limit in die Surface-Konfiguration ein. Anders
    // ohne Treiber-Rueckfrage moeglich (s. GpuState::set_frame_latency).
    pub(crate) fn apply_frame_latency(&mut self) {
        let wanted = self.settings.frame_latency;
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.set_frame_latency(wanted);
        }
    }

    // Die Schriften, die HUD-Zahlen (Schild-Countdown, Pfeile) oder Bereiche (Chat, Inventar,
    // ...) statt der allgemeinen wollen -- dafuer haelt GpuState weitere Atlanten bereit (s.
    // GpuState::ensure_fonts).
    pub(crate) fn wanted_fonts(&self) -> Vec<FontKind> {
        let set = &self.settings;
        let allgemein = FontKind::from_index(set.font);
        let mut fonts: Vec<FontKind> = Vec::new();
        // Der Schrift-Tab zeigt jede Schrift zur Probe.
        let probe = (self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsFont).then_some(FontKind::ALL).into_iter().flatten();
        // Kleine Beschriftungen stehen nie in der Minecraft-Schrift (s. GpuState::small_label_font).
        let klein = (allgemein == FontKind::Minecraft).then_some(FontKind::Outfit);
        for f in ShadowArea::ALL.into_iter().filter_map(|a| set.font_of(a)).chain(probe).chain(klein) {
            if f != allgemein && !fonts.contains(&f) {
                fonts.push(f);
            }
        }
        fonts
    }
}

// Config, die so heisst wie der Server: Name in der Serverliste, Adresse oder Adresse ohne
// Port (Configs duerfen keinen Doppelpunkt enthalten). Gross/klein egal, in dieser Reihenfolge.
fn server_config_for(configs: &[String], name: Option<&str>, address: &str) -> Option<String> {
    let host = address.rsplit_once(':').filter(|(h, p)| !h.contains(':') && p.parse::<u16>().is_ok()).map_or(address, |(h, _)| h);
    [name.map(str::trim), Some(address), Some(host)]
        .into_iter()
        .flatten()
        .filter(|k| !k.is_empty())
        .find_map(|k| configs.iter().find(|c| c.eq_ignore_ascii_case(k)))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::server_config_for;

    fn configs() -> Vec<String> {
        ["Standard", "Hypixel", "pvp.land", "localhost"].map(String::from).to_vec()
    }

    #[test]
    fn server_config_name_address_host() {
        let c = configs();
        assert_eq!(server_config_for(&c, Some("hypixel"), "mc.hypixel.net").as_deref(), Some("Hypixel"));
        assert_eq!(server_config_for(&c, Some("Land"), "PvP.land").as_deref(), Some("pvp.land"));
        assert_eq!(server_config_for(&c, None, "pvp.land:25565").as_deref(), Some("pvp.land"));
        assert_eq!(server_config_for(&c, None, "localhost:25566").as_deref(), Some("localhost"));
        assert_eq!(server_config_for(&c, Some("Irgendwas"), "example.org"), None);
        assert_eq!(server_config_for(&c, Some(""), ""), None);
    }
}
