// Anbindung der Addons (s. addons.rs): Laden, Hooks, Abfragen, Aktionen, Einstellungen und HUD.

use crate::*;
use coral_addon as api;

impl App {
    // Addon, Moduldefinition und Config-Schluessel ("addon/modul") eines Addon-Moduls.
    pub(crate) fn addon_def(&self, m: u16) -> Option<(&addons::Addon, &addons::ModuleDef, String)> {
        let (a, mi, key) = addon_module_info(m, |i| (i.addon, i.module, i.key.clone()))?;
        let addon = self.addons.as_ref()?.list.get(a)?;
        Some((addon, addon.modules.get(mi)?, key))
    }

    pub(crate) fn addon_setting(&self, m: u16, k: u16) -> Option<addons::SettingDef> {
        self.addon_def(m)?.1.settings.get(k as usize).cloned()
    }

    // Stand einer Einstellung als Text: gespeichert, sonst der Startwert des Addons.
    pub(crate) fn addon_value(&self, m: u16, k: u16) -> String {
        let Some((_, modul, key)) = self.addon_def(m) else { return String::new() };
        let Some(def) = modul.settings.get(k as usize) else { return String::new() };
        self.settings.addon_values.get(&format!("{key}/{}", def.key)).cloned().unwrap_or_else(|| def.default.clone())
    }

    // Einstellung setzen und dem Addon melden.
    pub(crate) fn set_addon_value(&mut self, m: u16, k: u16, value: String) {
        let Some(a) = addon_module_info(m, |i| i.addon) else { return };
        let Some((_, modul, key)) = self.addon_def(m) else { return };
        let modul = modul.id.clone();
        let Some(def) = self.addon_setting(m, k) else { return };
        self.settings.addon_values.insert(format!("{key}/{}", def.key), value.clone());
        self.addon_send(a, api::Event::Setting { module: modul, key: def.key, value: addons::value_of(&def.kind, &value) });
    }

    pub(crate) fn set_addon_enabled(&mut self, m: u16, on: bool) {
        let Some(a) = addon_module_info(m, |i| i.addon) else { return };
        let Some((_, modul, key)) = self.addon_def(m) else { return };
        let modul = modul.id.clone();
        self.settings.addon_values.insert(format!("{key}/enabled"), on.to_string());
        self.addon_send(a, api::Event::ModuleEnabled { module: modul, on });
    }

    // Nach der Anmeldung: dem Addon den gespeicherten Stand aller seiner Module schicken.
    pub(crate) fn send_addon_state(&mut self, a: usize) {
        let Some(addon) = self.addons.as_ref().and_then(|x| x.list.get(a)) else { return };
        let mut ereignisse = Vec::new();
        for modul in &addon.modules {
            let key = format!("{}/{}", addon.id, modul.id);
            let an = self.settings.addon_values.get(&format!("{key}/enabled")).is_some_and(|v| v == "true");
            ereignisse.push(api::Event::ModuleEnabled { module: modul.id.clone(), on: an });
            for def in modul.settings.iter().filter(|d| !matches!(d.kind, addons::SettingKind::Button)) {
                let wert = self.settings.addon_values.get(&format!("{key}/{}", def.key)).cloned().unwrap_or_else(|| def.default.clone());
                ereignisse.push(api::Event::Setting { module: modul.id.clone(), key: def.key.clone(), value: addons::value_of(&def.kind, &wert) });
            }
        }
        for e in ereignisse {
            self.addon_send(a, e);
        }
    }

    // Kategorien und Module aller angemeldeten Addons fuer ModuleId/ModuleCategory, dazu die
    // Startwerte fuer Call::GetSetting und die Schalter der Paket-Hooks.
    pub(crate) fn rebuild_addon_registry(&mut self) {
        let (mut kategorien, mut module) = (Vec::new(), Vec::new());
        self.addon_setting_defs.clear();
        for (a, addon) in self.addons.iter().flat_map(|x| x.list.iter()).enumerate() {
            if !addon.registered || addon.dead {
                continue;
            }
            for m in &addon.modules {
                for d in &m.settings {
                    self.addon_setting_defs.insert(format!("{}/{}/{}", addon.id, m.id, d.key), d.clone());
                }
            }
            if addon.modules.is_empty() {
                continue;
            }
            let k = kategorien.len() as u16;
            kategorien.push(intern(&addon.name));
            for (mi, m) in addon.modules.iter().enumerate() {
                module.push(AddonModuleInfo { label: intern(&m.name), description: intern(&m.description), category: k, key: format!("{}/{}", addon.id, m.id), addon: a, module: mi });
            }
        }
        if let Ok(mut c) = ADDON_CATEGORIES.write() {
            *c = kategorien;
        }
        if let Ok(mut m) = ADDON_MODULES.write() {
            *m = module;
        }
        let wants = |h| self.addons.as_ref().is_some_and(|x| x.any_wants(h));
        self.addon_raw_in.store(wants(api::Hook::PacketIn), std::sync::atomic::Ordering::Relaxed);
        let halten = wants(api::Hook::PacketOut);
        if let Some(tx) = self.outgoing_tx.as_ref() {
            tx.hold(halten);
        }
        self.addon_hold_packets = halten;
    }

    // Addons beim ersten Durchlauf laden, danach fertig uebersetzte anmelden.
    pub(crate) fn drain_addons(&mut self) {
        if self.addons.is_none() {
            let Some(dir) = assets::user_dir().map(|d| d.join("mods")) else { return };
            match addons::start(&dir) {
                Ok(x) => self.addons = Some(x),
                Err(e) => {
                    self.push_log(format!("Addons nicht verfuegbar: {e}"));
                    return;
                }
            }
        }
        let neu = self.addons.as_mut().map(|x| x.poll()).unwrap_or_default();
        for r in neu {
            match r {
                Ok(a) => self.init_addon(a),
                Err(e) => self.push_log(e),
            }
        }
    }

    fn init_addon(&mut self, a: usize) {
        let Some(mut x) = self.addons.take() else { return };
        let addon = &mut x.list[a];
        let antwort = addon.call(&*self, &api::Event::Init { api: api::API_VERSION, client: env!("CARGO_PKG_VERSION").into() });
        let id = addon.id.clone();
        let aktionen = addon.take_queued();
        let meldung = match antwort {
            Ok(api::Reply::Registration(reg)) => {
                addon.apply_registration(reg);
                format!("Addon {id} angemeldet ({} Module)", addon.modules.len())
            }
            Ok(_) => {
                addon.dead = true;
                format!("Addon {id} hat sich nicht angemeldet")
            }
            Err(e) => format!("Addon {id} startet nicht: {e}"),
        };
        self.addons = Some(x);
        self.push_log(meldung);
        self.rebuild_addon_registry();
        self.send_addon_state(a);
        for c in aktionen {
            self.addon_apply(a, c);
        }
    }

    // Ein Event an genau ein Addon (Stand der Module, Knopf, Befehl).
    pub(crate) fn addon_send(&mut self, a: usize, event: api::Event) -> api::Flow {
        let Some(mut x) = self.addons.take() else { return api::Flow::Continue };
        let Some(addon) = x.list.get_mut(a) else {
            self.addons = Some(x);
            return api::Flow::Continue;
        };
        let antwort = addon.call(&*self, &event);
        let aktionen = addon.take_queued();
        let id = addon.id.clone();
        self.addons = Some(x);
        let flow = match antwort {
            Ok(api::Reply::Flow(f)) => f,
            Ok(_) => api::Flow::Continue,
            Err(e) => {
                self.addon_failed(&id, &e);
                api::Flow::Continue
            }
        };
        for c in aktionen {
            self.addon_apply(a, c);
        }
        flow
    }

    // Ein Addon mit Modulen ist nur an, solange mindestens eins davon an ist: dann bekommt es
    // keine Hooks und keine Befehle, und was es an Tasten drueckt, zaehlt nicht. Eines ohne
    // Module ist immer an.
    pub(crate) fn addon_active(&self, addon: &addons::Addon) -> bool {
        addon.modules.is_empty()
            || addon.modules.iter().any(|m| self.settings.addon_values.get(&format!("{}/{}/enabled", addon.id, m.id)).is_some_and(|v| v == "true"))
    }

    // Ein Hook an alle Addons, die ihn hoeren, in Ladereihenfolge. Ein Abbruch endet die
    // Kette; ein ersetzter Inhalt geht ans naechste Addon weiter und kommt am Ende zurueck.
    // Waehrend der Aufrufe ist self.addons leer -- loest eine Aktion denselben Hook erneut aus
    // (z.B. Call::Chat -> ChatOut), laeuft er deshalb nicht im Kreis.
    pub(crate) fn addon_hook(&mut self, mut event: api::Event) -> api::Flow {
        let Some(hook) = event.hook() else { return api::Flow::Continue };
        if !self.addons.as_ref().is_some_and(|x| x.any_wants(hook)) {
            return api::Flow::Continue;
        }
        let aktiv: Vec<bool> = self.addons.iter().flat_map(|x| x.list.iter()).map(|a| self.addon_active(a)).collect();
        let Some(mut x) = self.addons.take() else { return api::Flow::Continue };
        let mut flow = api::Flow::Continue;
        let mut aktionen = Vec::new();
        let mut fehler = Vec::new();
        for (a, addon) in x.list.iter_mut().enumerate() {
            if !addon.wants(hook) || !aktiv[a] {
                continue;
            }
            let antwort = addon.call(&*self, &event);
            aktionen.extend(addon.take_queued().into_iter().map(|c| (a, c)));
            match antwort {
                Ok(api::Reply::Flow(api::Flow::Cancel)) => {
                    flow = api::Flow::Cancel;
                    break;
                }
                Ok(api::Reply::Flow(api::Flow::ReplaceText(t))) => match &mut event {
                    api::Event::ChatIn { text } | api::Event::ChatOut { text } => {
                        text.clone_from(&t);
                        flow = api::Flow::ReplaceText(t);
                    }
                    _ => {}
                },
                Ok(api::Reply::Flow(api::Flow::ReplaceBytes(b))) => {
                    if let api::Event::PacketOut { id, data } = &mut event {
                        if let Ok(neu) = protocol::packets::peek_packet_id(&b) {
                            *id = neu;
                            data.clone_from(&b);
                            flow = api::Flow::ReplaceBytes(b);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => fehler.push((addon.id.clone(), e)),
            }
        }
        self.addons = Some(x);
        for (id, e) in fehler {
            self.addon_failed(&id, &e);
        }
        for (a, c) in aktionen {
            self.addon_apply(a, c);
        }
        flow
    }

    fn addon_failed(&mut self, id: &str, e: &str) {
        self.push_log(format!("Addon {id} abgeschaltet: {e}"));
        self.notify(notifications::Notification::info(format!("Addon {id} abgeschaltet"), e.to_string()));
        self.rebuild_addon_registry();
    }

    // Tick-Hook (s. update_movement), vor dem Spieler-Tick.
    pub(crate) fn addon_tick(&mut self) {
        self.addon_hook(api::Event::Tick);
    }

    // Einmal je Durchlauf der Hauptschleife: Frame-Hook, eigener Tod (Hook::Death) und die
    // zurueckgehaltenen Pakete.
    pub(crate) fn addon_frame(&mut self) {
        let jetzt = Instant::now();
        let dt = jetzt.duration_since(self.addon_frame_at).as_secs_f32();
        self.addon_frame_at = jetzt;
        if self.session.deaths > self.addon_deaths_seen {
            self.addon_hook(api::Event::Death);
        }
        self.addon_deaths_seen = self.session.deaths;
        self.addon_hook(api::Event::Frame { dt });
        self.flush_addon_packets();
    }

    // Eigener Befehl eines Addons? Dann geht er an das Addon statt an den Server.
    pub(crate) fn addon_command(&mut self, text: &str) -> bool {
        let Some(zeile) = text.strip_prefix('/') else { return false };
        let (name, args) = zeile.split_once(' ').unwrap_or((zeile, ""));
        let name = name.to_lowercase();
        let Some(a) =
            self.addons.as_ref().and_then(|x| x.list.iter().position(|a| a.registered && !a.dead && a.commands.contains(&name) && self.addon_active(a)))
        else {
            return false;
        };
        self.addon_send(a, api::Event::Command { name, args: args.to_string() });
        true
    }

    // Ausgehendes Paket durch Hook::PacketOut. Nur aktiv, waehrend ein Addon ihn hoert: dann
    // sammelt PacketTx die Pakete, und sie gehen hier gefiltert raus (s. main_loop).
    pub(crate) fn flush_addon_packets(&mut self) {
        let Some(gehalten) = self.outgoing_tx.as_ref().map(|tx| tx.take_held()) else { return };
        for paket in gehalten {
            let id = protocol::packets::peek_packet_id(&paket).unwrap_or(-1);
            let raus = match self.addon_hook(api::Event::PacketOut { id, data: paket.clone() }) {
                api::Flow::Cancel => None,
                api::Flow::ReplaceBytes(b) => Some(b),
                _ => Some(paket),
            };
            if let (Some(p), Some(tx)) = (raus, self.outgoing_tx.as_ref()) {
                let _ = tx.send_direct(p);
            }
        }
    }

    // Was ein Addon ausgeloest hat, nach dem Hook.
    pub(crate) fn addon_apply(&mut self, a: usize, call: api::Call) {
        let Some(id) = self.addons.as_ref().and_then(|x| x.list.get(a)).map(|x| x.id.clone()) else { return };
        match call {
            api::Call::Hud { module, items } => {
                let module = addons::sanitize_id(&module);
                if let Some(m) = self.addons.as_mut().and_then(|x| x.list.get_mut(a)).and_then(|x| x.modules.iter_mut().find(|m| m.id == module)) {
                    m.hud = items;
                }
            }
            api::Call::Chat { text } => {
                // Hoechstens zwei Zeilen je Sekunde -- ein Addon in einer Schleife soll einen
                // nicht vom Server werfen lassen.
                if self.addon_chat_at.is_some_and(|t| t.elapsed() < Duration::from_millis(500)) {
                    self.push_log(format!("[{id}] Chat verworfen (zu schnell): {text}"));
                } else if !text.trim().is_empty() {
                    self.addon_chat_at = Some(Instant::now());
                    self.send_chat_text(text);
                }
            }
            api::Call::Print { text } => self.push_chat_message(vec![protocol::nbt::TextSpan { text, ..Default::default() }]),
            api::Call::Notify { title, text } => {
                self.notify(notifications::Notification::info(title, text));
            }
            api::Call::Sound { event, volume, pitch } => {
                let event = if event.contains(':') { event } else { format!("minecraft:{event}") };
                self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
                let seed = (zufallswert(self.type_pitch_seed) * i64::MAX as f32) as i64;
                if let Some(aufnahme) = self.vanilla_sounds.resolve(&mut self.installation, &event, seed) {
                    let gain = aufnahme.volume * volume.max(0.0) * self.settings.sound_gain(protocol::packets::SoundCategory::Master);
                    self.audio.sink().play_sample(aufnahme.sample, gain, 0.0, aufnahme.pitch * pitch.clamp(0.5, 2.0));
                }
            }
            api::Call::Log { text } => self.push_log(format!("[{id}] {text}")),
            api::Call::SetSetting { module, key, value } => {
                let schluessel = format!("{id}/{}/{}", addons::sanitize_id(&module), addons::sanitize_id(&key));
                // Ohne Echo: das Addon kennt den Wert schon.
                if self.addon_setting_defs.contains_key(&schluessel) {
                    self.settings.addon_values.insert(schluessel, addons::value_text(&value));
                    self.persist_settings();
                }
            }
            api::Call::SendPacket { data } => {
                if protocol::packets::peek_packet_id(&data).is_ok() {
                    if let Some(tx) = self.outgoing_tx.as_ref() {
                        let _ = tx.send_direct(data);
                    }
                }
            }
            api::Call::SetRotation { yaw, pitch } => {
                if yaw.is_finite() && pitch.is_finite() {
                    self.camera.yaw = yaw;
                    self.camera.pitch = pitch.clamp(-MAX_PITCH, MAX_PITCH);
                }
            }
            api::Call::SelectSlot { slot } => self.select_hotbar_slot((slot as usize).min(8)),
            api::Call::Attack => self.attack_click(),
            api::Call::Use => {
                self.apply_key_action(KeyBindAction::Use, true);
                self.apply_key_action(KeyBindAction::Use, false);
            }
            api::Call::Drop { whole_stack } => self.queue_input_action(PendingInputAction::DropItem(whole_stack)),
            api::Call::SwapHands => self.queue_input_action(PendingInputAction::SwapOffhand),
            api::Call::ClickSlot { slot, button, mode } => self.send_container_click(slot, button, mode),
            api::Call::SetInput { input } => {
                if input == api::Input::default() {
                    self.addon_inputs.remove(&a);
                } else {
                    self.addon_inputs.insert(a, input);
                }
            }
            api::Call::CloseScreen => {
                if self.screen_overlay_open() || self.chat_input.is_some() {
                    self.on_escape();
                }
            }
            // Abfragen beantwortet Host::answer schon waehrend des Aufrufs.
            api::Call::Player | api::Call::Inventory | api::Call::Entities { .. } | api::Call::Block { .. } | api::Call::Target | api::Call::Players | api::Call::Screen | api::Call::GetSetting { .. } => {}
        }
    }

    // Die Bewegungstasten, die Addons gerade druecken (s. Call::SetInput, movement_input_flags).
    pub(crate) fn addon_input(&self) -> api::Input {
        let aktiv = |a: &usize| self.addons.as_ref().and_then(|x| x.list.get(*a)).is_some_and(|addon| self.addon_active(addon));
        self.addon_inputs.iter().filter(|(a, _)| aktiv(a)).map(|(_, i)| i).fold(api::Input::default(), |s, i| api::Input {
            forward: s.forward || i.forward,
            back: s.back || i.back,
            left: s.left || i.left,
            right: s.right || i.right,
            jump: s.jump || i.jump,
            sneak: s.sneak || i.sneak,
            sprint: s.sprint || i.sprint,
        })
    }

    fn addon_player(&self) -> api::PlayerState {
        let in_welt = self.mode == AppMode::World && self.outgoing_tx.is_some();
        let p = &self.physics;
        api::PlayerState {
            in_world: in_welt,
            name: self.own_name().unwrap_or_default().to_string(),
            entity_id: self.own_entity_id.unwrap_or(-1),
            x: p.pos.x,
            y: p.pos.y,
            z: p.pos.z,
            vx: p.vel.x,
            vy: p.vel.y,
            vz: p.vel.z,
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            on_ground: p.on_ground,
            sprinting: p.sprinting,
            sneaking: self.sneak_held,
            health: self.health,
            food: self.food_level,
            saturation: self.food_saturation,
            xp_level: self.xp_level,
            xp_progress: self.xp_progress,
            game_mode: self.game_mode,
            selected_slot: self.inventory.held_slot as u8,
            dimension: self.dimension_type_name.clone(),
            server: self.server_address.clone(),
        }
    }

    fn addon_entities(&self, radius: f64) -> Vec<api::EntityInfo> {
        let p = self.physics.pos;
        let eigen = self.own_entity_id;
        let mut liste: Vec<api::EntityInfo> = self
            .entities
            .iter()
            .filter(|(id, _)| Some(**id) != eigen)
            .filter_map(|(&id, e)| {
                let (dx, dy, dz) = (e.pos.0 - p.x, e.pos.1 - p.y, e.pos.2 - p.z);
                let distance = (dx * dx + dy * dy + dz * dz).sqrt();
                if distance > radius {
                    return None;
                }
                let typ = world::entity::entity_type_name(e.entity_type).unwrap_or("unknown");
                let name = if typ == "player" {
                    self.tab_entries.get(&e.uuid).map(|t| t.name.clone()).unwrap_or_default()
                } else {
                    e.custom_name.as_ref().map(|s| s.iter().map(|s| s.text.as_str()).collect()).unwrap_or_default()
                };
                Some(api::EntityInfo { id, kind: namespaced(typ), name, x: e.pos.0, y: e.pos.1, z: e.pos.2, distance })
            })
            .collect();
        liste.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        liste
    }
}

fn namespaced(name: &str) -> String {
    if name.contains(':') {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    }
}

fn addon_item(s: &protocol::item::ItemStack) -> Option<api::Item> {
    let name = world::item_registry::item_name(s.item_id)?;
    Some(api::Item { id: namespaced(name), count: s.count, damage: s.damage })
}

impl addons::Host for App {
    fn answer(&self, addon: &str, call: &api::Call) -> api::Answer {
        match call {
            api::Call::Player => api::Answer::Player(self.addon_player()),
            api::Call::Inventory => api::Answer::Inventory(self.inventory.player.iter().map(|s| s.as_ref().and_then(addon_item)).collect()),
            api::Call::Entities { radius } => api::Answer::Entities(self.addon_entities(radius.clamp(0.0, 512.0))),
            api::Call::Block { x, y, z } => {
                let geladen = self.world.column(x.div_euclid(16), z.div_euclid(16)).is_some();
                api::Answer::Block(geladen.then(|| world::block_registry::block_name(self.world.block_at(*x, *y, *z))).flatten().map(namespaced))
            }
            api::Call::Target => api::Answer::Target(match self.crosshair_target() {
                CrosshairTarget::Entity(id) => api::Target::Entity { id },
                CrosshairTarget::Block((x, y, z), face) => {
                    let block = world::block_registry::block_name(self.world.block_at(x, y, z)).map(namespaced).unwrap_or_default();
                    api::Target::Block { x, y, z, face, block }
                }
                CrosshairTarget::Miss => api::Target::Miss,
            }),
            api::Call::Players => api::Answer::Players(
                self.tab_entries.iter().filter(|(_, e)| e.listed).map(|(uuid, e)| api::PlayerEntry { name: e.name.clone(), uuid: *uuid, ping: e.latency, game_mode: e.game_mode }).collect(),
            ),
            api::Call::Screen => {
                let (width, height) = self.gpu.as_ref().map(|g| g.size()).unwrap_or((0.0, 0.0));
                api::Answer::Screen(api::ScreenState { width, height, hud_scale: self.hud_scale((width, height)), screen_open: self.screen_overlay_open() || self.mode != AppMode::World, chat_open: self.chat_input.is_some() })
            }
            api::Call::GetSetting { module, key } => {
                let schluessel = format!("{addon}/{}/{}", addons::sanitize_id(module), addons::sanitize_id(key));
                let Some(def) = self.addon_setting_defs.get(&schluessel) else { return api::Answer::Value(api::Value::None) };
                let text = self.settings.addon_values.get(&schluessel).cloned().unwrap_or_else(|| def.default.clone());
                api::Answer::Value(addons::value_of(&def.kind, &text))
            }
            _ => api::Answer::None,
        }
    }
}

impl App {
    // Die Detailzeilen eines Addon-Moduls aus seinen angemeldeten Einstellungen.
    pub(crate) fn push_addon_rows(&self, m: u16, rows: &mut Vec<ModuleRow>) {
        let Some((_, modul, _)) = self.addon_def(m) else { return };
        for (k, def) in modul.settings.iter().enumerate() {
            let k = k as u16;
            let wert = self.addon_value(m, k);
            rows.push(match &def.kind {
                addons::SettingKind::Toggle => ModuleRow::Toggle { label: def.label.clone(), action: ButtonAction::AddonToggle(m, k), on: wert == "true" },
                addons::SettingKind::Slider { min, max, unit, .. } => {
                    let v = wert.parse::<f64>().unwrap_or(*min).clamp(*min, *max);
                    ModuleRow::Stepper {
                        label: def.label.clone(),
                        prev: ButtonAction::AddonStep(m, k, false),
                        next: ButtonAction::AddonStep(m, k, true),
                        text: format!("{} {unit}", addon_number_text(v)).trim_end().to_string(),
                        can_prev: v > *min,
                        can_next: v < *max,
                        slider: Some((v, *min, *max)),
                    }
                }
                addons::SettingKind::Choice { .. } => ModuleRow::Choice { label: def.label.clone(), action: ButtonAction::OpenChoice(Choice::Addon(m, k)), text: wert },
                addons::SettingKind::Color => {
                    let c = addons::parse_color(&wert).unwrap_or(0xFF_FFFF);
                    let [r, g, b, _] = theme::rgba(c);
                    ModuleRow::Swatch { label: def.label.clone(), action: ButtonAction::OpenColorPicker(ColorTarget::Addon(m, k)), rgb: [r, g, b] }
                }
                addons::SettingKind::Button => ModuleRow::Choice { label: def.label.clone(), action: ButtonAction::AddonButton(m, k), text: "Ausführen".to_string() },
            });
        }
    }

    // Klicks auf Addon-Zeilen (s. apply_module_action).
    pub(crate) fn addon_action(&mut self, action: ButtonAction) {
        match action {
            ButtonAction::ToggleAddonModule(m) => {
                let an = ModuleId::Addon(m).enabled(&self.settings);
                self.set_addon_enabled(m, !an);
            }
            ButtonAction::AddonToggle(m, k) => {
                let an = self.addon_value(m, k) == "true";
                self.set_addon_value(m, k, (!an).to_string());
            }
            ButtonAction::AddonStep(m, k, hoch) => {
                let Some(addons::SettingKind::Slider { min, max, step, .. }) = self.addon_setting(m, k).map(|d| d.kind) else { return };
                let v = self.addon_value(m, k).parse::<f64>().unwrap_or(min);
                let neu = ((v + if hoch { step } else { -step }) / step).round() * step;
                self.set_addon_value(m, k, addon_number_text(neu.clamp(min, max)).replace(',', "."));
            }
            ButtonAction::AddonButton(m, k) => {
                let Some(a) = addon_module_info(m, |i| i.addon) else { return };
                let Some((_, modul, _)) = self.addon_def(m) else { return };
                let modul = modul.id.clone();
                let Some(def) = self.addon_setting(m, k) else { return };
                self.addon_send(a, api::Event::Button { module: modul, key: def.key });
            }
            _ => {}
        }
    }

    // Was die Addon-Module ins HUD zeichnen, in GUI-Einheiten (mal HUD-Massstab) ab ihrer Ecke.
    pub(crate) fn build_addon_hud(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        let Some(x) = self.addons.as_ref() else { return };
        let Ok(module) = ADDON_MODULES.read() else { return };
        let (sw, sh) = gpu.size();
        let s = self.hud_scale((sw, sh));
        let ort = |anker: addons::Anchor, x: f32, y: f32, w: f32, h: f32| -> (f32, f32) {
            let (x, y) = (x * s, y * s);
            match anker {
                addons::Anchor::TopLeft => (x, y),
                addons::Anchor::TopRight => (sw - x - w, y),
                addons::Anchor::BottomLeft => (x, sh - y - h),
                addons::Anchor::BottomRight => (sw - x - w, sh - y - h),
                addons::Anchor::Center => ((sw - w) / 2.0 + x, (sh - h) / 2.0 + y),
            }
        };
        for (i, info) in module.iter().enumerate() {
            if !ModuleId::Addon(i as u16).enabled(&self.settings) {
                continue;
            }
            let Some(modul) = x.list.get(info.addon).filter(|a| !a.dead).and_then(|a| a.modules.get(info.module)) else { continue };
            for item in &modul.hud {
                match item {
                    addons::HudItem::Text { text, x, y, color, anchor } => {
                        let (w, h) = (gpu.measure_text(text, UI_SCALE_SMALL), gpu.text_height(UI_SCALE_SMALL));
                        let (px, py) = ort(*anchor, *x, *y, w, h);
                        texts.push(UiText { text: text.clone(), x: px.round(), y: py.round(), color: theme::rgba(*color), scale: UI_SCALE_SMALL });
                    }
                    addons::HudItem::Rect { x, y, w, h, color, alpha, anchor } => {
                        let (w, h) = (w * s, h * s);
                        let (px, py) = ort(*anchor, *x, *y, w, h);
                        rects.push(UiRect { rect: Rect { x: px, y: py, w, h }, color: theme::with_alpha(theme::rgba(*color), alpha.clamp(0.0, 1.0)), ..UiRect::FLAT });
                    }
                    addons::HudItem::Item { id, count, x, y, anchor } => {
                        let Some(item) = world::item_registry::item_id_by_name(id.trim_start_matches("minecraft:")) else { continue };
                        let groesse = 16.0 * s;
                        let (px, py) = ort(*anchor, *x, *y, groesse, groesse);
                        let stapel = protocol::item::ItemStack::new(item, (*count).max(1));
                        self.push_item_icon_only(sprites, &stapel, px.round(), py.round(), groesse);
                        if *count > 1 {
                            self.push_count_label(texts, &count.to_string(), px.round(), py.round(), groesse);
                        }
                    }
                }
            }
        }
    }
}
