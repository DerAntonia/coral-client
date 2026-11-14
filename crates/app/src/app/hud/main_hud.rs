// Das HUD-Bild, Sichtbarkeit des Fadenkreuzes, Skalen und Item-Name ueber der Hotbar.

use super::*;

impl App {
    // Letzter Rueckgabewert: der Sprite-Index, vor dem die Spielerfigur im Inventar gezeichnet
    // werden muss (s. build_inventory_screen). None = keine Figur in diesem Frame.
    pub(crate) fn build_hud(&self) -> (Vec<UiRect>, Vec<UiLine>, Vec<UiText>, Vec<UiSprite>, Option<usize>) {
        let Some(gpu) = self.gpu.as_ref() else {
            return (Vec::new(), Vec::new(), Vec::new(), Vec::new(), None);
        };
        let mut sprites: Vec<UiSprite> = Vec::new();
        let mut rects = Vec::new();
        let mut hud_lines = Vec::new();
        let mut texts = Vec::new();
        // Das Feuer vor den Augen zuerst: es liegt unter allem anderen (s. under_sprites).
        self.build_fire_overlay(gpu, &mut sprites);
        gpu.under_sprites.set(sprites.len());
        // Tearing-Test im Spiel, ganz hinten ueber der Welt. Nur im World-Modus: im Menue ruft
        // build_menu_ui diese Funktion selbst auf und hat die Balken schon gezeichnet.
        if self.mode == AppMode::World {
            let (sw, sh) = gpu.size();
            self.push_tearing_bars(&mut rects, sw, sh);
            self.push_profiler_overlay(gpu, &mut rects, &mut texts);
        }
        // Verbindungs-Overlay: nach "Los" landet man sofort in der 3D-Ansicht, aber solange
        // noch kein Chunk da ist, sitzt man vor einer leeren Welt. Dieses Overlay zeigt bis
        // dahin die Connect-Phase (aus dem Log). Unabhaengig von show_hud -- es ist keine
        // Performance-Anzeige, sondern eine Zustandsmeldung.
        if self.connecting_view() {
            self.build_connecting_overlay(gpu, &mut rects, &mut texts);
            // Dialoge des Servers (etwa der Verhaltenskodex in der Konfigurationsphase) kommen
            // vor dem ersten Chunk -- ohne sie hinge die Verbindung unsichtbar.
            if self.dialog_shown() || self.code_of_conduct.is_some() {
                self.build_dialog_ui(gpu, &mut rects, &mut texts, &mut sprites);
            }
            return (rects, hud_lines, texts, sprites, None);
        }
        // Was zur WELT gehoert (Namensschilder, Chat, Scoreboard, Hotbar, Totem-Anzeige),
        // verschwindet hinter einem Bildschirm-Overlay -- es wird gar nicht erst gebaut.
        //
        // Reihenfolge allein reicht dafuer NICHT: der UI-Pass zeichnet TYPWEISE gebatcht
        // (erst alle Flaechen, dann alle Sprites, dann aller Text, s. render_world). Ein
        // Namensschild ist Text, die Menue-Tafel eine Flaeche -- der Text liegt damit IMMER
        // oben, egal in welcher Reihenfolge beide in die Listen geschrieben werden. Ein
        // frueherer Versuch, das per Sortierung zu loesen, konnte deshalb gar nicht wirken
        // (Nametags blieben durch das Modul-Menue sichtbar).
        //
        // Das Inventar bleibt bewusst milder: dort blendet Vanilla den Chat weiter ein, nur
        // die Weltbeschriftungen sind verdeckt.
        //
        // Die Taste "HUD verstecken" (Vanillas F1, s. hud_hidden) nimmt im Spiel alles davon
        // weg. Der Chat kommt zurueck, sobald man tippt -- sonst saehe man nicht, was man
        // schreibt. Inventar und Modul-Menue bleiben, die hat man ja gerade selbst geoeffnet.
        let versteckt = self.hud_hidden && self.mode == AppMode::World;
        let welt_hud_verdeckt = self.module_menu_open || (versteckt && self.chat_input.is_none());
        // Schreibt man direkt aufs Schild (s. sign_edit_in_world), bleibt die Welt wie sie ist.
        let schild_in_welt = self.sign_edit_in_world(gpu).is_some();
        let beschriftungen_verdeckt = (self.screen_overlay_open() && !schild_in_welt) || versteckt;
        if !beschriftungen_verdeckt {
            self.build_totem_indicator(gpu, &mut texts, &mut sprites);
        }
        if self.settings.show_hud && !versteckt {
            self.push_hud_perf_texts(gpu, COLOR_PERF, &mut texts);
        }
        if self.debug_overlay && self.mode == AppMode::World && !versteckt {
            self.build_debug_overlay(gpu, &mut rects, &mut hud_lines, &mut texts);
        }
        if self.mode == AppMode::World {
            // Erst build_player_list traegt sie wieder ein, falls sie in diesem Bild steht.
            self.player_list_rect.set(None);
            // Bei offenem Inventar zeigt Vanilla die HUD-Hotbar nicht mehr -- sie steckt
            // dann als eigene Slot-Reihe im Bildschirm selbst.
            if !beschriftungen_verdeckt {
                self.begin_text_area(gpu, &mut texts, ShadowArea::Hotbar);
                self.build_hotbar(gpu, &mut sprites, &mut texts);
                Self::end_text_area(gpu, &mut texts);
                self.build_arrow_hud(gpu, &mut sprites, &mut texts);
                self.begin_text_area(gpu, &mut texts, ShadowArea::Hotbar);
                self.build_armor_hud(gpu, &mut rects, &mut sprites, &mut texts);
                Self::end_text_area(gpu, &mut texts);
                if self.settings.module_keystrokes && self.conn_status == ConnStatus::Connected {
                    self.push_keystrokes(gpu, &mut rects, &mut texts);
                }
                if self.settings.module_session_info && self.conn_status == ConnStatus::Connected {
                    self.push_session_info(gpu, &mut rects, &mut texts);
                }
                if self.settings.module_clock && self.conn_status == ConnStatus::Connected {
                    self.push_clock(gpu, &mut texts);
                }
                self.build_toteming_hud(gpu, &mut rects, &mut hud_lines, &mut sprites, &mut texts);
                self.build_gamemode_wheel(gpu, &mut hud_lines, &mut sprites, &mut texts);
                // Gui.renderHotbarAndDecorations: Herzen, Ruestung, Hunger und Luft nur, wenn
                // man Schaden nehmen kann, die Erfahrungsleiste nur mit Erfahrung -- beides gilt
                // fuer Ueberleben und Abenteuer, nicht fuer Kreativ und Zuschauer.
                if !matches!(self.game_mode, GAME_MODE_CREATIVE | GAME_MODE_SPECTATOR) {
                    self.build_status_bars(gpu, &mut sprites, &mut texts);
                    if self.settings.module_saturation_hud {
                        self.build_saturation_bar(gpu, &mut rects);
                    }
                    if self.settings.hotbar_air_bar {
                        self.build_air_bar(gpu, &mut rects);
                    }
                }
                self.begin_text_area(gpu, &mut texts, ShadowArea::Hotbar);
                self.build_held_name(gpu, &mut rects, &mut texts);
                Self::end_text_area(gpu, &mut texts);
                self.build_boss_bars(gpu, &self.boss_bars, &mut sprites, &mut texts);
                self.build_voice_hud(gpu, &mut texts);
                if self.settings.show_effects_hud && self.conn_status == ConnStatus::Connected {
                    self.build_effect_hud(gpu, &mut sprites, &mut texts, false, 1.0);
                }
                self.build_sound_meter(gpu, &mut rects, &mut texts);
                self.build_world_labels(gpu, &mut rects, &mut texts);
            }
            // Schilder gehoeren zur Welt, nicht zum HUD: auch bei F1 und unter Fenstern.
            self.build_sign_texts(gpu);
            if !welt_hud_verdeckt {
                // Der Chat ist fuer die Textauswahl ein eigener Bereich (und darin Verlauf,
                // Eingabe und Liste der gesendeten Nachrichten je einer, s. selection_boundary).
                selection_boundary(&mut texts);
                self.begin_text_area(gpu, &mut texts, ShadowArea::Chat);
                self.build_chat_overlay(gpu, &mut rects, &mut texts);
                Self::end_text_area(gpu, &mut texts);
                selection_boundary(&mut texts);
                self.build_server_hud(gpu, &mut rects, &mut texts);
                if self.player_list_held && self.conn_status == ConnStatus::Connected {
                    self.begin_text_area(gpu, &mut texts, ShadowArea::PlayerList);
                    self.build_player_list(gpu, &mut rects, &mut sprites, &mut texts);
                    Self::end_text_area(gpu, &mut texts);
                }
            }
        }
        // BILDSCHIRM-OVERLAYS GANZ ZULETZT (s. screen_overlay_open): Inventar und
        // Modul-Menue liegen ueber ALLEM, auch ueber Namensschildern und Chat.
        //
        // Der UI-Pass hat keinen Tiefentest -- allein die Reihenfolge in diesen Vecs
        // entscheidet, was oben liegt (s. render_world). Frueher standen die beiden Overlays
        // weiter oben, und build_world_labels/build_chat_overlay haben danach in dieselben Vecs
        // geschrieben: ein Namensschild eines Spielers hinter dem Menue wurde dadurch MITTEN
        // INS Menue gezeichnet, ebenso jede eintreffende Chatzeile.
        //
        // Was Addon-Module zeichnen (s. build_addon_hud), unter Menues und Overlays. Nur in der
        // Welt und nicht per F1 versteckt -- show_hud ist der Schalter der FPS-Anzeige.
        if self.mode == AppMode::World && !versteckt {
            self.build_addon_hud(gpu, &mut rects, &mut texts, &mut sprites);
            self.build_replay_bar(gpu, &mut rects, &mut texts);
        }
        // Das Abdunkeln des Menues (s. build_module_menu) deckt jetzt ebenfalls Chat und
        // Namensschilder mit ab -- genau so soll es sein: ein Overlay verdeckt die Welt
        // vollstaendig, nicht nur die Haelfte davon.
        let mut doll_split = None;
        if self.mode == AppMode::World {
            if self.book_edit.is_some() {
                self.build_book_edit_screen(gpu, &mut rects, &mut sprites, &mut texts);
            } else if self.lectern_book().is_some() || self.book_view.is_some() {
                self.build_book_screen(gpu, &mut rects, &mut sprites, &mut texts);
            } else if self.inventory.screen_open {
                self.begin_text_area(gpu, &mut texts, ShadowArea::Inventory);
                doll_split = self.build_inventory_screen(gpu, &mut rects, &mut sprites, &mut texts);
                Self::end_text_area(gpu, &mut texts);
            }
            if self.sign_edit.is_some() {
                self.build_sign_editor(gpu, &mut rects, &mut sprites, &mut texts);
            }
            if self.module_menu_open {
                // In Menue-Pixeln gebaut, dann in echte umgerechnet (s. GpuState::ui_scale).
                let s = Self::world_menu_scale(gpu);
                let ab = (rects.len(), texts.len());
                let vorher = gpu.ui_scale.replace(s);
                self.build_module_menu(gpu, &mut rects, &mut texts);
                // Das Zeilenband der Detailansicht (s. module_detail_band) in echten Pixeln, nur fuer
                // den Scissor beim Zeichnen.
                crate::store_module_clip(self.module_detail_cut().then(|| crate::scale_rect(self.module_detail_band(gpu), s)));
                gpu.ui_scale.set(vorher);
                crate::scale_ui_tail(&mut rects, &mut texts, ab, s / vorher);
                // Das Modul-Menue legt sich ueber alles -- dann waere die Figur ein Stueck
                // 3D-Geometrie mitten darin.
                doll_split = None;
            }
            // Dialoge des Servers liegen ueber allem.
            if self.dialog_shown() || self.code_of_conduct.is_some() {
                self.build_dialog_ui(gpu, &mut rects, &mut texts, &mut sprites);
                doll_split = None;
            }
        }
        // Gehaltene Reconnect-Taste: das Bild dunkelt bis zum Ausloesen ab, wie osus Neustart.
        if let Some(f) = self.reconnect_progress() {
            let (sw, sh) = gpu.size();
            rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: [0.0, 0.0, 0.0, f], ..UiRect::FLAT });
        }
        self.push_selection_rects(gpu, &mut rects);
        (rects, hud_lines, texts, sprites, doll_split)
    }

    // Das Entity, durch dessen Augen ein Zuschauer gerade sieht -- solange es noch da ist.
    pub(crate) fn spectated_entity(&self) -> Option<&world::Entity> {
        self.entities.get(self.camera_entity?)
    }

    // Ob das Fadenkreuz gerade gezeichnet wird. Eine Quelle fuer Kontur (UI-Pass) und
    // Fadenkreuz-Pass, damit nie die Kontur ohne das Kreuz stehenbleibt.
    pub(crate) fn crosshair_visible(&self) -> bool {
        // Bei offener Chat-Eingabe ebenfalls unsichtbar: die Eingabezeile sitzt jetzt genau
        // an der Fadenkreuz-Position (s. chat_input_rect) -- beides gleichzeitig saehe aus,
        // als stuende das Kreuz mitten im Text. Als Zuschauer gar nicht.
        self.mode == AppMode::World
            && self.game_mode != GAME_MODE_SPECTATOR
            && (self.perspective.is_first_person() || self.shoulder_active())
            // Freelook: die Bildmitte ist nicht, wohin der Spieler zielt.
            && !self.freelook_held
            && !self.screen_overlay_open()
            && self.chat_input.is_none()
            && !self.hud_hidden
            && !self.connecting_view()
            && !self.player_list_over_crosshair()
    }

    // Reicht die Tab-Liste ueber das Fadenkreuz (Mitte bzw. Zielpunkt der Schulterkamera,
    // samt halber Groesse der Form)? Dann verschwindet es, statt durch die Namen zu stechen.
    // Die Flaeche stammt aus dem HUD-Bau dieses Bildes (build_hud laeuft vor dem
    // Fadenkreuz-Pass); was davor im selben Bau fragt, sieht die des letzten Bildes.
    fn player_list_over_crosshair(&self) -> bool {
        let (Some(liste), Some(gpu)) = (self.player_list_rect.get(), self.gpu.as_ref()) else { return false };
        let (sw, sh) = gpu.size();
        let (fx, fy) = self.shoulder_crosshair_pos().unwrap_or((0.5, 0.5));
        let halb = self.settings.crosshair_size() as f32 / 2.0;
        let (cx, cy) = (fx * sw, fy * sh);
        cx + halb > liste.x && cx - halb < liste.x + liste.w && cy + halb > liste.y && cy - halb < liste.y + liste.h
    }

    // Verbindungsbildschirm: im Spiel, aber noch ohne einen Chunk vom Server. Dahinter steht
    // die zuletzt gespeicherte Welt dieses Servers (s. lod_preview_saved) wie ein Standbild --
    // abgedunkelt, ohne HUD, die Kamera fest; der Knopf Abbrechen trennt.
    pub(crate) fn connecting_view(&self) -> bool {
        self.mode == AppMode::World && self.chunks_received == 0
    }

    // --- Hotbar und Inventar ------------------------------------------------------------

    // Beschriftung eines Skalierungswerts: 0 zeigt zusaetzlich, was Auto gerade ergibt --
    // sonst waere "Auto" eine Angabe, die man nicht nachpruefen kann.
    pub(crate) fn scale_label(setting: u32, screen: (f32, f32)) -> String {
        if setting == 0 {
            format!("Auto (x{})", inventory::gui_scale(screen) as u32)
        } else {
            format!("x{setting}")
        }
    }

    // GUI-Skalierung aus einer Einstellung: 0 heisst Auto (Vanillas "Auto", so gross wie das
    // 320x240-Grundraster erlaubt), sonst die feste Stufe, hoechstens inventory::max_gui_scale.
    pub(crate) fn gui_scale_of(setting: u32, screen: (f32, f32)) -> f32 {
        if setting == 0 {
            inventory::gui_scale(screen)
        } else {
            setting.clamp(1, inventory::max_gui_scale(screen)) as f32
        }
    }

    // Groesste feste Stufe fuer die Plus-Knoepfe der Skalierungen.
    pub(crate) fn max_gui_scale_setting(&self) -> u32 {
        self.gpu.as_ref().map_or(inventory::GUI_SCALE_MIN_MAX, |g| inventory::max_gui_scale(g.real_size()))
    }

    // Skalierung des Inventarfensters bzw. der Hotbar -- zwei getrennte Einstellungen, weil
    // man die Hotbar im Kampf gern klein haelt, das Inventar zum Sortieren aber gross.
    pub(crate) fn inventory_scale(&self, screen: (f32, f32)) -> f32 {
        Self::gui_scale_of(self.settings.inventory_scale, screen)
    }

    // Gui.tick fuer den Itemnamen ueber der Hotbar: ein anderes Item oder ein anderer Name
    // setzt ihn auf zwei Sekunden, sonst zaehlt er herunter. Anders als in Vanilla nur fuer
    // Items, die einen Tooltip haetten (s. item_needs_tooltip) -- welches gewoehnliche Item man
    // haelt, zeigt schon die Hotbar.
    pub(crate) fn tick_held_name(&mut self) {
        let schluessel = self
            .own_hand_stack(protocol::packets::HAND_MAIN)
            .filter(|s| !s.tooltip_hidden && item_shows_held_name(s))
            .map(|s| {
                let name: String = self.held_name_spans(s).iter().map(|t| t.text.as_str()).collect();
                format!("{}:{name}", s.item_id)
            });
        match schluessel {
            None => self.held_name_timer = 0,
            Some(ref k) if self.held_name_last.as_ref() != Some(k) => self.held_name_timer = HELD_NAME_TICKS,
            Some(_) => self.held_name_timer = (self.held_name_timer - 1).max(0),
        }
        self.held_name_last = schluessel;
    }

    // Der Name eines Items, wie ihn der Tooltip in seiner ersten Zeile zeigt (eigener Name,
    // Seltenheitsfarbe, Buchtitel ...).
    pub(crate) fn held_name_spans(&self, stack: &protocol::item::ItemStack) -> Vec<protocol::nbt::TextSpan> {
        protocol::nbt::with_lang_area(protocol::nbt::LangArea::Items, || self.item_tooltip_lines_in_area(stack)).into_iter().next().unwrap_or_default()
    }

    // Gui.renderSelectedItemName: mittig ueber der Hotbar, im Kreativmodus tiefer (dort fehlen
    // Herzen und Ruestung), in den letzten zehn Ticks ausblendend.
    pub(crate) fn build_held_name(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        if self.held_name_timer <= 0 || self.conn_status != ConnStatus::Connected {
            return;
        }
        let Some(stack) = self.own_hand_stack(protocol::packets::HAND_MAIN) else { return };
        let spans = self.held_name_spans(stack);
        if spans.is_empty() {
            return;
        }
        // Nur den Namen, fuer den der Timer laeuft: nach einem Slotwechsel steht bis zum
        // naechsten Tick sonst der Name des neuen Items da, auch wenn es gar keinen zeigt.
        let name: String = spans.iter().map(|t| t.text.as_str()).collect();
        if stack.tooltip_hidden || !item_shows_held_name(stack) || self.held_name_last.as_deref() != Some(format!("{}:{name}", stack.item_id).as_str()) {
            return;
        }
        let alpha = (self.held_name_timer as f32 * 256.0 / 10.0).min(255.0) / 255.0;
        let g = self.held_name_scale(gpu);
        let Rect { x, y, .. } = self.held_name_rect_for(gpu, span_row_width(gpu, &spans, g));
        let vorher = texts.len();
        push_styled_row(gpu, &spans, x, y, COLOR_WHITE, g, texts, &mut |r, mut c| {
            c[3] *= alpha;
            rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT });
        });
        // Marker tragen ihren Code in einer negativen Deckkraft und bleiben unberuehrt.
        for t in texts[vorher..].iter_mut().filter(|t| t.color[3] > 0.0) {
            t.color[3] *= alpha;
        }
    }

    // Schriftfaktor des Itemnamens: Hotbar-Skala mal eigener Prozentwert, auf Viertel gerundet
    // (wie title_scale).
    pub(crate) fn held_name_scale(&self, gpu: &GpuState) -> f32 {
        let g = self.hotbar_scale(gpu.size()) * self.settings.hud_held_name_scale as f32 / 100.0;
        (g * 4.0).round().max(1.0) / 4.0
    }

    // Wo ein Itemname der Breite `w` steht. Ungezogen an der Hotbar wie in Vanilla (mittig, 37
    // ueber ihrer Oberkante, im Kreativmodus tiefer), sonst um die gespeicherte Mitte.
    pub(crate) fn held_name_rect_for(&self, gpu: &GpuState, w: f32) -> Rect {
        let (sw, sh) = gpu.size();
        let h = gpu.text_height(self.held_name_scale(gpu));
        let set = &self.settings;
        let (x, y) = if set.hud_held_name_y < 0.0 {
            let hotbar = self.hud_hotbar_rect(gpu);
            let g = self.hotbar_scale(gpu.size());
            let ueber = if matches!(self.game_mode, GAME_MODE_CREATIVE | GAME_MODE_SPECTATOR) { HELD_NAME_ABOVE_HOTBAR - 14.0 } else { HELD_NAME_ABOVE_HOTBAR };
            (hotbar.x + (hotbar.w - w) / 2.0, hotbar.y - ueber * g)
        } else {
            (set.hud_held_name_x * sw - w / 2.0, set.hud_held_name_y * sh)
        };
        Rect { x: x.clamp(0.0, (sw - w).max(0.0)).round(), y: y.clamp(0.0, (sh - h).max(0.0)).round(), w, h }
    }

    // Griff im HUD-Editor: der Name des gehaltenen Items, ohne einen ein Platzhalter.
    pub(crate) fn hud_held_name_rect(&self, gpu: &GpuState) -> Rect {
        let g = self.held_name_scale(gpu);
        let w = match self.own_hand_stack(protocol::packets::HAND_MAIN) {
            Some(stack) => span_row_width(gpu, &self.held_name_spans(stack), g),
            None => gpu.measure_text("Itemname", g),
        };
        self.held_name_rect_for(gpu, w.max(gpu.measure_text("Itemname", g)))
    }

    pub(crate) fn hotbar_scale(&self, screen: (f32, f32)) -> f32 {
        Self::gui_scale_of(self.settings.hud_hotbar_scale, screen)
    }

    // Grundskala der HUD-Teile, die nicht an der Hotbar haengen (Effekte, Pfeile, Spielerliste,
    // Bossleisten ...): Vanillas Auto-Stufe, damit sie nicht mitwachsen, wenn man nur die
    // Hotbar groesser oder kleiner stellt.
    pub(crate) fn hud_scale(&self, screen: (f32, f32)) -> f32 {
        Self::gui_scale_of(0, screen)
    }

    // Textskalierung des Chats: Prozent von UI_SCALE_SMALL, auf Viertel gerundet -- die
    // Schrift liegt in Viertel-Fassungen vor (s. font_atlas::DM_SANS_QUARTERS) und bleibt nur
    // dann pixelgenau.
    pub(crate) fn chat_scale(&self) -> f32 {
        let roh = UI_SCALE_SMALL * self.settings.chat_scale_percent as f32 / 100.0;
        ((roh * 4.0).round() / 4.0).max(0.5)
    }
}
