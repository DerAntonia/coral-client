// Editierbare Werte, Regler und Stufen-Regler.

use super::*;

impl App {
    // Welches numerische Wertfeld (falls ueberhaupt) unter (px,py) liegt -- s.
    // EditableValue. Ein Screen kann mehrere haben (SettingsGeneral hat vier), deshalb wird
    // ueber alle gesucht statt pro Screen genau eines zurueckzugeben.
    pub(crate) fn editable_value_at(&self, px: f32, py: f32) -> Option<EditableValue> {
        EditableValue::ALL.into_iter().find(|ev| {
            ev.screen() == self.screen && self.editable_shown(*ev) && if ev.slider().is_some() { ev.slider_value_rect() } else { ev.rect() }.contains(px, py)
        })
    }

    // Steht das Wertfeld gerade da? Die Staerke eines Schatten-Kaestchens nur, wenn es gefuellt
    // ist, und je nach Art die Dicke (Kontur) oder die Deckkraft (alle ausser "Aus").
    pub(crate) fn editable_shown(&self, ev: EditableValue) -> bool {
        match ev {
            EditableValue::ShadowThickness(i) => self.settings.shadow_slot_style(i) == Some(TextShadow::Outline),
            EditableValue::ShadowOpacity(i) => {
                matches!(self.settings.shadow_slot_style(i), Some(TextShadow::Vanilla | TextShadow::Below | TextShadow::Around))
            }
            _ => true,
        }
    }

    // Freitext-Bearbeitung eines Zahlenfelds beginnen -- Startwert ist der aktuell
    // angezeigte (formatierte) Wert, damit man ihn bequem nachbearbeiten statt komplett neu
    // eintippen kann.
    pub(crate) fn begin_edit_value(&mut self, ev: EditableValue) {
        // Puffer startet LEER, nicht mit dem bisherigen Wert: fast immer will man eine neue
        // Zahl tippen, nicht die alte nachbearbeiten -- sonst muesste man sie erst
        // wegloeschen. Der bisherige Wert geht dabei nicht verloren: er steht weiter
        // abgeblendet im Feld (s. draw_editable_value), und ein leer bestaetigter Puffer
        // laesst ihn unveraendert stehen (commit_edit_value verwirft Unparsbares).
        self.editing_value = Some((ev, String::new()));
    }

    // Freitext-Eingabe in einem Regler ueber Pfeil-Aktionen beginnen -- leer wie bei
    // begin_edit_value; ein zweiter Klick mitten in der Eingabe setzt sie nicht zurueck.
    pub(crate) fn begin_step_edit(&mut self, prev: ButtonAction) {
        if self.editing_step.as_ref().map(|(p, _)| *p) != Some(prev) {
            self.editing_step = Some((prev, String::new()));
        }
    }

    // Puffer parsen, klemmen (dieselben Grenzen wie der jeweilige Stepper) und uebernehmen
    // -- ein nicht parsbarer Puffer (leer, nur ein Punkt, ...) wird stillschweigend
    // verworfen, der alte Wert bleibt stehen, statt auf 0 zu springen.
    pub(crate) fn commit_edit_value(&mut self) {
        let Some((ev, buf)) = self.editing_value.take() else {
            return;
        };
        self.apply_edit_buffer(ev, &buf);
        self.persist_settings();
    }

    // Eine Zahl (als Text in Anzeige-Einheiten) uebernehmen, mit denselben Grenzen wie der
    // jeweilige Stepper. Ohne Speichern -- das macht der Aufrufer (beim Ziehen am Regler erst
    // beim Loslassen).
    pub(crate) fn apply_edit_buffer(&mut self, ev: EditableValue, buf: &str) {
        // "-1" heisst bei den Obergrenzen aus EditableValue::allows_unlimited "unbegrenzt".
        // Die Renderdistanz gehoert nicht dazu: ihr Maximum ist eine echte Chunk-Zahl.
        let unlimited = ev.allows_unlimited() && buf.trim() == UNLIMITED_INPUT;
        match ev {
            EditableValue::MouseSensitivity => {
                // Puffer zeigt den *1000-skalierten Anzeigewert (s. begin_edit_value) --
                // zurueck in Radiant/Pixel umrechnen.
                if let Ok(v) = buf.parse::<f32>() {
                    self.settings.mouse_sensitivity = v / 1000.0;
                    self.settings.couple_sensitivity(false);
                    self.sens_last_changed_y = false;
                }
            }
            EditableValue::MouseSensitivityY => {
                if let Ok(v) = buf.parse::<f32>() {
                    self.settings.mouse_sensitivity_y = v / 1000.0;
                    self.settings.couple_sensitivity(true);
                    self.sens_last_changed_y = true;
                }
            }
            EditableValue::CursorSensitivity => {
                // Der Puffer zeigt den Faktor unskaliert (s. begin_edit_value) -- direkt
                // uebernehmen, kein 1000er-Faktor wie bei der Kamera.
                if let Ok(v) = buf.parse::<f32>() {
                    self.settings.cursor_sensitivity = v;
                }
            }
            EditableValue::MeshBudget => {
                if let Ok(v) = buf.parse::<usize>() {
                    self.settings.mesh_columns_per_frame = v;
                }
            }
            EditableValue::Fov(p) => {
                if let Ok(v) = buf.parse::<f32>() {
                    match p {
                        Perspective::FirstPerson => self.settings.fov = v,
                        Perspective::ThirdPersonBack => self.settings.fov_third_back = v,
                        Perspective::ThirdPersonFront => self.settings.fov_third_front = v,
                    }
                }
            }
            EditableValue::CameraDistance(p) => {
                if let Ok(v) = buf.parse::<f32>() {
                    match p {
                        Perspective::FirstPerson => {}
                        Perspective::ThirdPersonBack => self.settings.camera_distance_back = v,
                        Perspective::ThirdPersonFront => self.settings.camera_distance_front = v,
                    }
                }
            }
            EditableValue::CameraYOffset(p) => {
                if let Ok(v) = buf.parse::<f32>() {
                    match p {
                        Perspective::FirstPerson => {}
                        Perspective::ThirdPersonBack => self.settings.camera_y_offset_back = v,
                        Perspective::ThirdPersonFront => self.settings.camera_y_offset_front = v,
                    }
                }
            }
            EditableValue::FpsLimit => {
                if unlimited {
                    self.settings.fps_limit = FPS_LIMIT_UNLIMITED;
                } else if let Ok(v) = buf.parse::<u32>() {
                    self.settings.fps_limit = v;
                }
            }
            EditableValue::MaxRenderDistance => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.max_render_distance = v.clamp(MAX_RENDER_DISTANCE_MIN, MAX_RENDER_DISTANCE_MAX);
                }
            }
            EditableValue::InventoryTransparency => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.inventory_transparency = v;
                }
            }
            EditableValue::GlintSpeed => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.glint_speed = v;
                }
            }
            EditableValue::GlintStrength => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.glint_strength = v;
                }
            }
            EditableValue::PlayerListScale => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.player_list_scale = v;
                }
            }
            EditableValue::MenuBackdropOpacity => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.menu_backdrop_opacity = v;
                }
            }
            EditableValue::ScrollInertia => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.scroll_inertia = v.min(100);
                }
            }
            EditableValue::ScrollDistance => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.scroll_distance = v.clamp(SCROLL_DISTANCE_MIN, SCROLL_DISTANCE_MAX);
                }
            }
            EditableValue::EntityRenderDistance => {
                if unlimited {
                    self.settings.entity_render_distance = ENTITY_DISTANCE_MAX;
                } else if let Ok(v) = buf.parse::<u32>() {
                    self.settings.entity_render_distance = v;
                }
            }
            EditableValue::BlockEntityRenderDistance => {
                if unlimited {
                    self.settings.block_entity_render_distance = BLOCK_ENTITY_DISTANCE_MAX;
                } else if let Ok(v) = buf.parse::<u32>() {
                    self.settings.block_entity_render_distance = v;
                }
            }
            EditableValue::CalcMinecraftSens => {
                if let Ok(v) = buf.parse::<f32>() {
                    self.settings.sens_calc_mc_percent = v;
                }
            }
            EditableValue::CalcDpi => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.sens_calc_dpi = v;
                }
            }
            EditableValue::MasterVolume => {
                if let Some(v) = parse_volume(buf) {
                    self.settings.master_volume = v;
                }
            }
            EditableValue::CategoryVolume(i) => {
                if let (Some(v), Some(feld)) = (parse_volume(buf), self.settings.category_volume.get_mut(i)) {
                    *feld = v;
                }
            }
            EditableValue::UiSoundVolume(i) => {
                if let (Some(v), Some(feld)) = (parse_volume(buf), self.settings.ui_sound_volume.get_mut(i)) {
                    *feld = v;
                }
            }
            EditableValue::ShadowThickness(i) => {
                if let (Ok(v), Some(feld)) = (buf.replace(',', ".").parse::<f32>(), self.settings.shadow_thickness.get_mut(i)) {
                    // Nur was ein u8 fassen kann.
                    *feld = (v * 10.0).round().clamp(0.0, f32::from(u8::MAX)) as u8;
                }
                self.sync_text_shadow();
            }
            EditableValue::ShadowOpacity(i) => {
                if let (Ok(v), Some(feld)) = (buf.parse::<u32>(), self.settings.shadow_opacity.get_mut(i)) {
                    *feld = v.min(u32::from(u8::MAX)) as u8;
                }
                self.sync_text_shadow();
            }
        }
    }

    // Aktueller Wert eines Zahlenfelds in Anzeige-Einheiten (wie apply_edit_buffer ihn liest).
    pub(crate) fn editable_number(&self, ev: EditableValue) -> f64 {
        let s = &self.settings;
        match ev {
            EditableValue::MouseSensitivity => f64::from(s.mouse_sensitivity) * 1000.0,
            EditableValue::MouseSensitivityY => f64::from(s.mouse_sensitivity_y) * 1000.0,
            EditableValue::CursorSensitivity => f64::from(s.cursor_sensitivity),
            EditableValue::MeshBudget => s.mesh_columns_per_frame as f64,
            EditableValue::Fov(p) => f64::from(s.fov_of(p)),
            EditableValue::CameraDistance(p) => f64::from(s.camera_distance_of(p)),
            EditableValue::CameraYOffset(p) => f64::from(s.camera_y_offset_of(p)),
            EditableValue::FpsLimit => if s.fps_limit == FPS_LIMIT_UNLIMITED { f64::from(FPS_LIMIT_TYPED_MAX) } else { f64::from(s.fps_limit) },
            EditableValue::MaxRenderDistance => f64::from(s.max_render_distance),
            EditableValue::InventoryTransparency => f64::from(s.inventory_transparency),
            EditableValue::GlintSpeed => f64::from(s.glint_speed),
            EditableValue::GlintStrength => f64::from(s.glint_strength),
            EditableValue::PlayerListScale => f64::from(s.player_list_scale),
            EditableValue::MenuBackdropOpacity => f64::from(s.menu_backdrop_opacity),
            EditableValue::ScrollInertia => f64::from(s.scroll_inertia),
            EditableValue::ScrollDistance => f64::from(s.scroll_distance),
            EditableValue::EntityRenderDistance => f64::from(s.entity_render_distance),
            EditableValue::BlockEntityRenderDistance => f64::from(s.block_entity_render_distance),
            EditableValue::CalcMinecraftSens => f64::from(s.sens_calc_mc_percent),
            EditableValue::CalcDpi => f64::from(s.sens_calc_dpi),
            EditableValue::MasterVolume => f64::from(s.master_volume),
            EditableValue::CategoryVolume(i) => f64::from(s.category_volume.get(i).copied().unwrap_or(0.0)),
            EditableValue::UiSoundVolume(i) => f64::from(s.ui_sound_volume.get(i).copied().unwrap_or(0.0)),
            EditableValue::ShadowThickness(i) => f64::from(s.shadow_thickness.get(i).copied().unwrap_or(SHADOW_THICKNESS_DEFAULT)) / 10.0,
            EditableValue::ShadowOpacity(i) => f64::from(s.shadow_opacity.get(i).copied().unwrap_or(100)),
        }
    }

    // Rechtes Ende eines Reglers: mit gehaltener rechter Taste das hoechste erlaubte, sonst das
    // normale -- aber nie unter dem aktuellen Wert (ein grosser Wert soll nicht am Rand kleben).
    pub(crate) fn slider_max(&self, ev: EditableValue) -> f64 {
        let Some(r) = ev.slider() else { return 1.0 };
        if self.slider_extended && self.slider_drag == Some(ev) {
            return r.hard;
        }
        r.soft.max(self.editable_number(ev).min(r.hard))
    }

    // Wert aus der Zeigerposition auf der Bahn setzen (Klick oder Ziehen).
    pub(crate) fn slider_set_from_pointer(&mut self, ev: EditableValue) {
        let Some(r) = ev.slider() else { return };
        let bahn = ev.slider_track();
        let t = f64::from(((self.pointer().0 - bahn.x) / bahn.w).clamp(0.0, 1.0));
        let max = self.slider_max(ev);
        let wert = r.min + t * (max - r.min);
        let text = format!("{wert:.*}", r.decimals);
        let vorher = self.editable_number(ev);
        self.apply_edit_buffer(ev, &text);
        let nachher = self.editable_number(ev);
        if nachher != vorher {
            self.play_slider_tick((nachher - r.min) / (max - r.min).max(1e-9));
        }
    }

    // Regler unter dem Zeiger (ganze Flaeche ausser der Wertanzeige).
    pub(crate) fn slider_at(&self, px: f32, py: f32) -> Option<EditableValue> {
        if self.mode != AppMode::Menu {
            return None;
        }
        EditableValue::ALL
            .into_iter()
            .filter(|ev| ev.screen() == self.screen && ev.slider().is_some() && self.editable_shown(*ev))
            .find(|ev| ev.slider_rect().contains(px, py) && !ev.slider_value_rect().contains(px, py))
    }

    // --- Schieberegler ueber Pfeil-Aktionen (s. StepSlider) ---

    pub(crate) fn step_slider_of(&self, prev: ButtonAction) -> Option<StepSlider> {
        let knoepfe = if self.module_menu_shown() {
            self.gpu.as_ref().map(|gpu| self.module_menu_buttons(gpu)).unwrap_or_default()
        } else {
            self.screen_buttons()
        };
        knoepfe.into_iter().filter_map(|b| b.slider).find(|s| s.prev == prev)
    }

    // Ein Schritt eines Schiebereglers: im Modul-Menue ueber dessen eigene Aktionen.
    pub(crate) fn run_step_action(&mut self, action: ButtonAction) {
        if self.module_menu_shown() {
            self.apply_module_action(action);
        } else {
            self.run_menu_action(action);
        }
    }

    // Den Wert unter den Zeiger bringen: Schritt fuer Schritt ueber die Pfeil-Aktionen, bis der
    // naechste Schritt weiter weg vom Ziel fuehren wuerde. Gespeichert wird erst beim
    // Loslassen (s. persist_paused).
    // Ziehen an einem Regler ueber Pfeil-Aktionen beginnen; `extended` = mit der rechten
    // Taste gefasst, die Bahn reicht dann bis zur harten Grenze.
    pub(crate) fn begin_step_drag(&mut self, sl: StepSlider, extended: bool) {
        self.step_drag = Some(sl.prev);
        self.step_extended = extended;
        self.step_drag_end = sl.end(extended);
        self.step_slider_drag();
    }

    // Rechtes Ende, mit dem ein Stufen-Regler gezeichnet wird: beim Ziehen das festgehaltene.
    pub(crate) fn step_slider_end(&self, sl: &StepSlider) -> f64 {
        if self.step_drag == Some(sl.prev) {
            self.step_drag_end
        } else {
            sl.end(false)
        }
    }

    // Angezeigte Daumen-Stelle eines Reglers: gleitet in einigen zehn Millisekunden zum Wert
    // `t`, statt hinzuspringen. Ein Regler, der eine Weile nicht gezeichnet wurde (andere
    // Seite, Fenster zu), steht sofort richtig.
    pub(crate) fn glide_slider(&self, key: SliderKey, t: f32) -> f32 {
        const ZEIT: f32 = 0.045;
        let jetzt = Instant::now();
        let mut liste = self.slider_glide.borrow_mut();
        liste.retain(|(_, _, zuletzt)| jetzt.duration_since(*zuletzt).as_secs_f32() < 1.0);
        let Some(eintrag) = liste.iter_mut().find(|(k, _, _)| *k == key) else {
            liste.push((key, t, jetzt));
            return t;
        };
        let dt = jetzt.duration_since(eintrag.2).as_secs_f32();
        let mut gezeigt = if dt > 0.2 { t } else { eintrag.1 + (t - eintrag.1) * (1.0 - (-dt / ZEIT).exp()) };
        if (t - gezeigt).abs() < 0.0005 {
            gezeigt = t;
        }
        *eintrag = (key, gezeigt, jetzt);
        gezeigt
    }

    pub(crate) fn step_slider_drag(&mut self) {
        let Some(prev) = self.step_drag else { return };
        let Some(sl) = self.step_slider_of(prev) else { return };
        let bahn = sl.track();
        let t = f64::from(((self.pointer().0 - bahn.x) / bahn.w).clamp(0.0, 1.0));
        let ende = self.step_drag_end;
        let ziel = sl.min + t * (ende - sl.min);
        self.persist_paused = true;
        let start = sl.value;
        // Ohne Schritte: der Wert unter dem Zeiger, ganzzahlige Felder gerundet.
        if self.set_step_value(prev, ziel) {
            self.persist_paused = false;
            if let Some(ende) = self.step_slider_of(prev).map(|s| s.value).filter(|&v| v != start) {
                self.play_slider_tick((ende - sl.min) / (self.step_drag_end - sl.min).max(1e-9));
            }
            return;
        }
        let mut wert = sl.value;
        for _ in 0..400 {
            let hoch = ziel > wert;
            if (ziel - wert).abs() < 1e-9 {
                break;
            }
            self.run_step_action(if hoch { sl.next } else { sl.prev });
            let Some(neu) = self.step_slider_of(prev).map(|s| s.value) else { break };
            let weiter = if hoch { neu > wert } else { neu < wert };
            if !weiter {
                break;
            }
            // Ueber das Ziel hinaus: zuruecknehmen, wenn der alte Wert naeher lag.
            if (hoch && neu > ziel) || (!hoch && neu < ziel) {
                if (neu - ziel).abs() > (wert - ziel).abs() {
                    self.run_step_action(if hoch { sl.prev } else { sl.next });
                }
                break;
            }
            wert = neu;
        }
        self.persist_paused = false;
        if let Some(ende) = self.step_slider_of(prev).map(|s| s.value).filter(|&v| v != start) {
            self.play_slider_tick((ende - sl.min) / (self.step_drag_end - sl.min).max(1e-9));
        }
    }

    // Regler ueber Pfeil-Aktionen unter dem Zeiger, sofern bedienbar.
    pub(crate) fn step_slider_at_pointer(&self) -> Option<StepSlider> {
        let knoepfe = if self.module_menu_shown() {
            self.gpu.as_ref().map(|gpu| self.module_menu_buttons(gpu)).unwrap_or_default()
        } else if self.mode == AppMode::Menu {
            self.screen_buttons()
        } else {
            return None;
        };
        let (px, py) = self.pointer();
        knoepfe.into_iter().filter(|b| b.enabled).filter_map(|b| b.slider).find(|s| s.rect().contains(px, py))
    }

    // Was im Feld eines Reglers ueber Pfeil-Aktionen steht: im Modul-Menue aus seinen Zeilen,
    // sonst aus dem Inhalt des Tabs, der den Wert ins alte Wertfeld schreibt (s. build_menu_ui).
    pub(crate) fn step_slider_text(&self, sl: &StepSlider) -> Option<String> {
        if self.module_menu_shown() {
            return self.module_detail_rows().into_iter().find_map(|r| match r {
                ModuleRow::Stepper { prev, text, .. } if prev == sl.prev => Some(text),
                _ => None,
            });
        }
        let gpu = self.gpu.as_ref()?;
        let (mut rects, mut texts, mut sprites) = (Vec::new(), Vec::new(), Vec::new());
        self.build_screen_content(gpu, &mut rects, &mut texts, &mut sprites);
        let feld = sl.value_rect;
        texts.into_iter().find(|t| feld.contains(t.x + 1.0, t.y + 1.0)).map(|t| t.text)
    }

    // Stand eines Reglers ueber Pfeil-Aktionen: innerer Wert und die Zahl im Feld. Ein Wort
    // im Feld ("Aus", "unbegrenzt") liegt vor bzw. hinter jeder Zahl.
    pub(crate) fn step_slider_reading(&self, prev: ButtonAction, uhr: bool) -> Option<(f64, f64)> {
        let sl = self.step_slider_of(prev)?;
        let zahl = match self.step_slider_text(&sl).as_deref().and_then(slider_number) {
            Some((n, true)) if uhr => clock_from_day_start(n),
            Some((n, _)) => n,
            None if sl.value <= sl.min => f64::NEG_INFINITY,
            None => f64::INFINITY,
        };
        Some((sl.value, zahl))
    }

    // Getippte Zahl in einen Regler ueber Pfeil-Aktionen uebernehmen: Schritt fuer Schritt, bis
    // das Feld sie zeigt -- gezaehlt in dem, was dort steht (Zehntel, Prozent, Uhrzeit), nicht
    // im inneren Wert. Grenzen und Schrittweite bleiben so die der Pfeile.
    pub(crate) fn commit_step_edit(&mut self) {
        let Some((prev, buf)) = self.editing_step.take() else { return };
        let Some((getippt, _)) = slider_number(&buf) else { return };
        let Some(sl) = self.step_slider_of(prev) else { return };
        let uhr = self.step_slider_text(&sl).as_deref().and_then(slider_number).is_some_and(|(_, u)| u);
        let ziel = if uhr { clock_from_day_start(getippt) } else { getippt };
        if let Some(innen) = self.display_to_step_value(prev, &sl, uhr, ziel) {
            if self.set_step_value(prev, innen) {
                self.persist_settings();
                return;
            }
        }
        let Some((mut wert, mut zahl)) = self.step_slider_reading(prev, uhr) else { return };
        self.persist_paused = true;
        for _ in 0..2000 {
            if (zahl - ziel).abs() < 1e-9 {
                break;
            }
            let hoch = ziel > zahl;
            let (hin, zurueck) = if hoch { (sl.next, sl.prev) } else { (sl.prev, sl.next) };
            self.run_step_action(hin);
            let Some((neu_wert, neu_zahl)) = self.step_slider_reading(prev, uhr) else { break };
            // Am Rand angekommen.
            if neu_wert == wert {
                break;
            }
            // Falsche Richtung (ein Pfeil, der am Ende umlaeuft): zuruecknehmen.
            if (hoch && neu_zahl < zahl) || (!hoch && neu_zahl > zahl) {
                self.run_step_action(zurueck);
                break;
            }
            // Ueber das Ziel hinaus: zuruecknehmen, wenn der alte Wert naeher lag.
            if (hoch && neu_zahl > ziel) || (!hoch && neu_zahl < ziel) {
                if (neu_zahl - ziel).abs() > (zahl - ziel).abs() {
                    self.run_step_action(zurueck);
                }
                break;
            }
            (wert, zahl) = (neu_wert, neu_zahl);
        }
        self.persist_paused = false;
        self.persist_settings();
    }

    // Regler, deren getippte Zahl genau so uebernommen wird statt ueber die Pfeilschritte
    // angefahren -- dort sind Werte zwischen den Schritten (Nachkommastellen) erlaubt.
    // Setzt den Wert eines Reglers ueber Pfeil-Aktionen direkt, in inneren Einheiten (dem, was
    // der Regler als Wert fuehrt) -- ohne Schritte und ohne Grenzen: was getippt oder gezogen
    // wird, bleibt so stehen. Ganzzahlige Felder runden auf die naechste ganze Zahl; was der Typ
    // nicht fassen kann (negativ in einem vorzeichenlosen Feld), wird verworfen. false = fuer
    // diesen Regler gibt es keinen direkten Zugriff.
    pub(crate) fn set_step_value(&mut self, prev: ButtonAction, v: f64) -> bool {
        if !self.set_step_quiet(prev, v) {
            return false;
        }
        self.after_step_value(prev);
        true
    }

    // Wie set_step_value, ohne die Nebenwirkungen der Pfeil-Aktion (s. after_step_value).
    pub(crate) fn set_step_quiet(&mut self, prev: ButtonAction, v: f64) -> bool {
        if !v.is_finite() {
            return false;
        }
        match prev {
            ButtonAction::EgoVolume(false) => self.settings.ego_volume = v as f32,
            ButtonAction::AddonStep(m, k, false) => {
                if !matches!(self.addon_setting(m, k).map(|d| d.kind), Some(addons::SettingKind::Slider { .. })) {
                    return false;
                }
                self.set_addon_value(m, k, addon_number_text(v).replace(',', "."));
            }
            ButtonAction::Toteming(toteming::Action::Volume(slot, _)) => {
                let Some(n) = num_u32(v) else { return false };
                self.settings.toteming.sound[slot.index()].volume = n;
            }
            ButtonAction::Toteming(toteming::Action::Pitch(slot, _)) => {
                let Some(n) = num_u32(v) else { return false };
                self.settings.toteming.sound[slot.index()].pitch = n;
            }
            _ => {
                let Some(feld) = self.step_field(prev) else { return false };
                if !feld.set(v) {
                    return false;
                }
            }
        }
        true
    }

    // Getippte Zahl (in dem, was im Feld steht: Zehntel, Prozent, Uhrzeit ...) in den inneren
    // Wert umrechnen. Die Anzeige ist je Regler linear im inneren Wert; zwei Probewerte ergeben
    // die Gerade, danach steht der alte Wert wieder. None = kein direkter Zugriff oder keine
    // zwei Zahlen im Feld.
    pub(crate) fn display_to_step_value(&mut self, prev: ButtonAction, sl: &StepSlider, uhr: bool, ziel: f64) -> Option<f64> {
        let vorher = sl.value;
        let mut punkte: Vec<(f64, f64)> = Vec::new();
        for t in [0.5, 1.0, 0.25, 0.0, 0.75] {
            let probe = sl.min + t * (sl.max - sl.min);
            if !self.set_step_quiet(prev, probe) {
                break;
            }
            let Some((w, z)) = self.step_slider_reading(prev, uhr) else { continue };
            if z.is_finite() && punkte.iter().all(|&(pw, pz)| pw != w && pz != z) {
                punkte.push((w, z));
            }
            if punkte.len() == 2 {
                break;
            }
        }
        self.set_step_quiet(prev, vorher);
        let [(w1, z1), (w2, z2)] = punkte[..] else { return None };
        Some(w1 + (ziel - z1) * (w2 - w1) / (z2 - z1))
    }

    // Das Feld hinter einem Regler ueber Pfeil-Aktionen (s. set_step_value).
    pub(crate) fn step_field(&mut self, prev: ButtonAction) -> Option<NumMut<'_>> {
        let set = &mut self.settings;
        Some(match prev {
            ButtonAction::EgoLight(schatten, false) => if schatten { (&mut set.ego_shadow_light).into() } else { (&mut set.ego_light).into() },
            ButtonAction::ShieldColorPrev(farbe, achse) => set.shield_color_mut(farbe, achse)?.into(),
            ButtonAction::HitboxDensityPrev => (&mut set.module_hitbox_density_px).into(),
            ButtonAction::HitboxOpacityPrev => (&mut set.module_hitbox_opacity).into(),
            ButtonAction::TeamOpacity(seite, false) => (&mut set.team_hitboxes[seite.index()].opacity).into(),
            ButtonAction::TeamDensity(seite, false) => (&mut set.team_hitboxes[seite.index()].density_px).into(),
            ButtonAction::TrailHoldPrev => (&mut set.trail_hold_tenths).into(),
            ButtonAction::TrailFadePrev => (&mut set.trail_fade_tenths).into(),
            ButtonAction::SkyTimePrev => (&mut set.module_sky_time).into(),
            ButtonAction::WeatherRadiusPrev => (&mut set.weather_radius).into(),
            ButtonAction::WeatherSnowRadiusPrev => (&mut set.weather_snow_radius).into(),
            ButtonAction::WeatherSnowRadiusMaxPrev => (&mut set.weather_snow_radius_max).into(),
            ButtonAction::WeatherDensityPrev => (&mut set.weather_density).into(),
            ButtonAction::WeatherSnowSpeedPrev => (&mut set.weather_snow_speed).into(),
            ButtonAction::ColorSaturationPrev => (&mut set.color_saturation).into(),
            ButtonAction::BlockOutlineWidthPrev => (&mut set.block_outline_width_px).into(),
            ButtonAction::VoiceStep(knob, false) => {
                let v = &mut set.voice;
                match knob {
                    VoiceKnob::Volume => (&mut v.volume_percent).into(),
                    VoiceKnob::MicGain => (&mut v.mic_gain_db).into(),
                    VoiceKnob::Threshold => (&mut v.threshold_db).into(),
                    VoiceKnob::Delay => if v.voice_activation { (&mut v.voice_delay_frames).into() } else { (&mut v.ptt_delay_frames).into() },
                    VoiceKnob::OutputBuffer => (&mut v.output_buffer_frames).into(),
                    VoiceKnob::PacketThreshold => (&mut v.packet_threshold).into(),
                }
            }
            ButtonAction::ZoomFactorPrev => (&mut set.zoom_factor_tenths).into(),
            ButtonAction::ClipSecondsPrev => (&mut set.clip_seconds).into(),
            ButtonAction::ClipCheckpointsPrev => (&mut set.clip_checkpoints).into(),
            ButtonAction::ZoomSensPrev => (&mut set.zoom_sens_percent).into(),
            ButtonAction::EatHoldThroughPrev => (&mut set.eat_hold_through_ms).into(),
            ButtonAction::NoToolBreakReservePrev => (&mut set.no_tool_break_reserve).into(),
            ButtonAction::SaturationHeightPrev => (&mut set.saturation_hud_height).into(),
            ButtonAction::SaturationAlpha(raster, false) => if raster { (&mut set.saturation_hud_grid_alpha).into() } else { (&mut set.saturation_hud_bg_alpha).into() },
            ButtonAction::EgoSpeed(false) => (&mut set.ego_speed).into(),
            ButtonAction::AutoclickerCpsPrev => (&mut set.autoclicker_cps).into(),
            ButtonAction::LagDelayPrev => (&mut set.lag_delay_ms).into(),
            ButtonAction::GhostTolerancePrev => (&mut set.ghost_tolerance_ms).into(),
            ButtonAction::Toteming(toteming::Action::Offset(_)) => (&mut set.toteming.offset).into(),
            ButtonAction::Toteming(toteming::Action::Size(_)) => (&mut set.toteming.size).into(),
            ButtonAction::ChunkUpdateThreadsDown => (&mut set.chunk_update_threads).into(),
            ButtonAction::TearTimingDown => (&mut set.tear_timing_us).into(),
            ButtonAction::TearReserveDown => (&mut set.tear_reserve_us).into(),
            ButtonAction::SmoothLightingDown => (&mut set.smooth_lighting_percent).into(),
            ButtonAction::ChunkFadeDown => (&mut set.chunk_fade_ms).into(),
            ButtonAction::ParticleFadeDown => (&mut set.particle_fade_out_ms).into(),
            ButtonAction::EntityNearDown => (&mut set.entity_near_dither_cm).into(),
            ButtonAction::MipSharpnessDown => (&mut set.mip_sharpness_pct).into(),
            ButtonAction::ParticleDistanceDown => (&mut set.particle_distance).into(),
            ButtonAction::MotionBlurDown => (&mut set.motion_blur_percent).into(),
            ButtonAction::EntityBlurDown => (&mut set.motion_blur_entities_percent).into(),
            ButtonAction::HandBlurDown => (&mut set.hand_blur_percent).into(),
            ButtonAction::SlotHoverFadeDown => (&mut set.slot_hover_fade_ms).into(),
            ButtonAction::SwapAnimDown => (&mut set.swap_anim_ms).into(),
            ButtonAction::HotbarGlideDown => (&mut set.hotbar_glide_ms).into(),
            ButtonAction::DamageTintTicksPrev => (&mut set.damage_tint_ticks).into(),
            ButtonAction::ChunkSaverThresholdPrev => (&mut set.chunk_saver_threshold).into(),
            ButtonAction::HurtCamStrengthPrev => (&mut set.hurt_cam_strength).into(),
            ButtonAction::PanoramaSpeedPrev => (&mut set.panorama_speed).into(),
            ButtonAction::ShoulderOffsetPrev => (&mut set.shoulder_offset_cm).into(),
            ButtonAction::FireHeightPrev => (&mut set.fire_height).into(),
            ButtonAction::ParticleStep(own, i, col, false) => {
                let t = set.particle_tuning_of_mut(own).get_mut(i)?;
                match col {
                    0 => (&mut t.lifetime_pct).into(),
                    1 => (&mut t.opacity_pct).into(),
                    2 => (&mut t.speed_pct).into(),
                    3 => (&mut t.size_pct).into(),
                    4 => (&mut t.spread_pct).into(),
                    5 if particles::KINDS[i].spawn.has_interval() => (&mut t.interval_pct).into(),
                    5 => (&mut t.amount_pct).into(),
                    _ => return None,
                }
            }
            ButtonAction::UiPitchDown => (&mut set.ui_pitch_semitones).into(),
            ButtonAction::CursorSizeDown => (&mut set.cursor_size).into(),
            ButtonAction::HoldConfirmDown => (&mut set.hold_confirm_ms).into(),
            ButtonAction::ReconnectHoldDown => (&mut set.reconnect_hold_ms).into(),
            ButtonAction::CursorWidthDown => (&mut set.cursor_width).into(),
            ButtonAction::CursorOutlineDown => (&mut set.cursor_outline).into(),
            ButtonAction::CursorFillDown => (&mut set.cursor_fill_transparency).into(),
            ButtonAction::CrosshairBoostPrev => (&mut set.crosshair_brightness_boost).into(),
            ButtonAction::CrosshairRingRadiusPrev => (&mut set.crosshair_ring_radius).into(),
            ButtonAction::CrosshairSizePrev => {
                let i = set.crosshair_shape.min(CROSSHAIR_SHAPES.len() - 1);
                (&mut set.crosshair_sizes[i]).into()
            }
            ButtonAction::HudScaleStep(element, false) => match element {
                HudElement::Title => (&mut set.hud_title_scale).into(),
                HudElement::Subtitle => (&mut set.hud_subtitle_scale).into(),
                HudElement::Effects => (&mut set.hud_effects_scale).into(),
                HudElement::BossBar => (&mut set.hud_bossbar_scale).into(),
                HudElement::Scoreboard => (&mut set.hud_scoreboard_scale).into(),
                HudElement::Arrows => (&mut set.hud_arrows_scale).into(),
                HudElement::HeldName => (&mut set.hud_held_name_scale).into(),
                HudElement::Armor => (&mut set.hud_armor_scale).into(),
                HudElement::Keystrokes => (&mut set.hud_keys_scale).into(),
                HudElement::Session => (&mut set.hud_session_scale).into(),
                HudElement::Clock => (&mut set.hud_clock_scale).into(),
                _ => return None,
            },
            ButtonAction::KeysOpacityPrev => (&mut set.keys_opacity).into(),
            ButtonAction::SessionOpacity(false) => (&mut set.session_opacity).into(),
            ButtonAction::ArmorHudGapPrev => (&mut set.armor_hud_gap).into(),
            ButtonAction::ArmorHudNumberScalePrev => (&mut set.armor_hud_number_scale).into(),
            ButtonAction::ArmorHudNumberXPrev => (&mut set.armor_hud_number_x).into(),
            ButtonAction::ArmorHudNumberYPrev => (&mut set.armor_hud_number_y).into(),
            ButtonAction::ArrowOpacityPrev => (&mut set.arrow_opacity).into(),
            ButtonAction::ChatScalePrev => (&mut set.chat_scale_percent).into(),
            ButtonAction::ChatWidthPrev => (&mut set.chat_width).into(),
            ButtonAction::ChatBackgroundPrev => (&mut set.chat_background).into(),
            ButtonAction::ChatOpenDimPrev => (&mut set.chat_open_dim).into(),
            ButtonAction::ChatLinesPrev => (&mut set.chat_lines).into(),
            ButtonAction::ChatLinesOpenPrev => (&mut set.chat_lines_open).into(),
            ButtonAction::ChatFadePrev => (&mut set.chat_fade_secs).into(),
            ButtonAction::ChatLineSpacingPrev => (&mut set.chat_line_spacing).into(),
            _ => return None,
        })
    }

    // Was die Pfeil-Aktion eines Reglers ausser dem Wert noch anstoesst, nach dem direkten Setzen.
    pub(crate) fn after_step_value(&mut self, prev: ButtonAction) {
        match prev {
            ButtonAction::ChunkUpdateThreadsDown => {
                self.mesh_pool.resize(self.settings.chunk_update_threads);
            }
            ButtonAction::VoiceStep(VoiceKnob::OutputBuffer, _) => self.audio.sink().set_stream_buffer(self.settings.voice.output_buffer_frames as usize),
            ButtonAction::VoiceStep(VoiceKnob::PacketThreshold, _) => {
                if let Some(c) = &self.voice.client {
                    c.set_packet_threshold(self.settings.voice.packet_threshold as usize);
                }
            }
            ButtonAction::LagDelayPrev => self.sync_lag_delay(),
            ButtonAction::WeatherSnowRadiusPrev | ButtonAction::WeatherSnowRadiusMaxPrev => self.settings.couple_snow_radius(),
            // Gezogen oder getippt: wie bei den Pfeilen steckt sie im Mesh (s. run_menu_action).
            ButtonAction::SmoothLightingDown => {
                for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
                    self.world.mark_dirty(cx, cz);
                }
            }
            _ => {}
        }
    }

    // Laufende Freitext-Eingabe in einem Wertfeld: der Puffer linksbuendig mit blinkendem
    // Caret (wie das Adressfeld auf dem Connect-Screen). Solange noch nichts getippt ist, steht
    // der bisherige Wert abgeblendet da (wie ein Platzhalter in einem Web-Formular): das Feld
    // startet leer, damit man sofort lostippen kann, aber man sieht weiterhin, was gerade
    // gilt -- und dass Enter ohne Eingabe genau diesen Wert stehen laesst.
    pub(crate) fn push_value_input(&self, gpu: &GpuState, rect: Rect, bisher: String, buf: &str, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let x = rect.x + 8.0;
        let y = (rect.y + (rect.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let (shown, color) = if buf.is_empty() { (bisher, theme::text_disabled()) } else { (buf.to_string(), theme::text()) };
        // Bei leerem Puffer sitzt der Caret VOR dem Platzhalter, sonst hinter dem Getippten --
        // in beiden Faellen dort, wo das naechste Zeichen landet.
        let caret_x = if buf.is_empty() { x } else { x + gpu.measure_raw(buf, UI_SCALE_SMALL) };
        let text_x = if buf.is_empty() { x + CARET_WIDTH_PX + 3.0 } else { x };
        push_typed_marker(texts);
        texts.push(UiText { text: shown, x: text_x, y, color, scale: UI_SCALE_SMALL });
        self.push_caret(rects, gpu.caret_rect(caret_x, y, UI_SCALE_SMALL));
    }

    // Rechtes Ende eines mit der rechten Taste gefassten Reglers, klein unter der Bahn.
    pub(crate) fn push_slider_max_label(&self, gpu: &GpuState, bahn: Rect, wert: String, texts: &mut Vec<UiText>) {
        let text = format!("max {wert}");
        let w = gpu.measure_text(&text, TOPBAR_SUB_SCALE);
        texts.push(UiText { text, x: (bahn.x + bahn.w - w).round(), y: (bahn.y + 9.0).round(), color: theme::text_3(), scale: TOPBAR_SUB_SCALE });
    }

    // Schieberegler zeichnen: Bahn, gefuellter Teil, Daumen (mit Ring beim Zeigen/Ziehen).
    pub(crate) fn push_slider_visual(rects: &mut Vec<UiRect>, bahn: Rect, t: f32, hover: bool, aktiv: bool, enabled: bool) {
        use theme::Slot;
        let g = theme::get;
        let t = t.clamp(0.0, 1.0);
        rects.push(UiRect::rounded(bahn, g(Slot::Surface3), 2.0));
        let fuell = if enabled { g(Slot::Accent) } else { g(Slot::Border2) };
        rects.push(UiRect::rounded(Rect { w: bahn.w * t, ..bahn }, fuell, 2.0));
        let daumen = Rect { x: (bahn.x + bahn.w * t - 5.0).round(), y: bahn.y + 2.0 - 10.0, w: 10.0, h: 20.0 };
        if hover && enabled {
            let ring = if aktiv { 6.0 } else { 4.0 };
            let a = if aktiv { 0.25 } else { 0.18 };
            rects.push(UiRect::rounded(
                Rect { x: daumen.x - ring, y: daumen.y - ring, w: daumen.w + 2.0 * ring, h: daumen.h + 2.0 * ring },
                theme::with_alpha(g(Slot::Accent), a),
                3.0 + ring,
            ));
        }
        rects.push(UiRect::shadow(daumen, 3.0, 1.0, 3.0, 0.6));
        rects.push(UiRect::rounded(daumen, if enabled { theme::rgba(0xF4F7FA) } else { g(Slot::TextDisabled) }, 3.0));
    }

    // --- Farbfenster ---
}
