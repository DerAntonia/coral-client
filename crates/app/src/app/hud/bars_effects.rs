// Hotbar, Lautstaerkemesser, Effekte, Saettigung und Luft.

use super::*;

impl App {
    // Den Zweithand-Slot neben der Hotbar (Vanillas hud/hotbar_offhand_*) zeichnen wir bewusst
    // nicht -- unnoetig, man sieht das Item in der Hand.
    pub(crate) fn build_hotbar(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        // Vor der ersten Inventar-Nachricht (oder ohne Verbindung) waere die Hotbar leer --
        // dann auch das Sheet weglassen, statt einen leeren Balken zu zeigen.
        // Zuschauer haben keine Hotbar (Gui: dort steht das Zuschauermenue).
        if self.conn_status != ConnStatus::Connected || self.game_mode == GAME_MODE_SPECTATOR {
            return;
        }
        let Some(hotbar) = self.ui_atlas.rect("hud/hotbar") else { return };
        let rect = self.hud_hotbar_rect(gpu);
        let scale = self.hotbar_scale(gpu.size());
        sprites.push(UiSprite::quad(rect.x, rect.y, rect.w, rect.h, hotbar.uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));

        for (index, slot) in inventory::hotbar_slots((rect.x, rect.y), scale).into_iter().enumerate() {
            if let Some(stack) = self.inventory.hotbar(index) {
                let grenze = if self.settings.hotbar_durability_absolute {
                    DurabilityBarLimit::Points(self.settings.hotbar_durability_points as i32)
                } else {
                    DurabilityBarLimit::Rest(self.settings.hotbar_durability_percent as f32 / 100.0)
                };
                self.push_item_limited(sprites, texts, stack, slot.x, slot.y, slot.size, grenze, self.hotbar_pop_amount(index));
            }
        }

        // Auswahlrahmen zuletzt, damit er ueber den Icons der Nachbarslots liegt (er ragt
        // einen Pixel darueber hinaus -- genau wie in Vanilla).
        // Mit "Auswahl gleitet" steht er zwischen den Slots; ueber den Rand hinaus wird er an der
        // Hotbar abgeschnitten und kommt auf der anderen Seite herein (s. hotbar_glide).
        if let Some(selection) = self.ui_atlas.rect("hud/hotbar_selection") {
            let lage = self.hotbar_glide.borrow_mut().step(self.inventory.held_slot, Instant::now(), self.settings.hotbar_glide_ms);
            let y = rect.y - scale;
            let size = inventory::HOTBAR_SELECTION * scale;
            let (links, rechts) = (rect.x - scale, rect.x + rect.w + scale);
            let [u0, v0, u1, v1] = selection.uv;
            // Die zweite Lage nur waehrend des Umlaufs: in 0..8 ragte ihr Rand sonst einen Spalt
            // weit auf die andere Seite.
            let umlauf = if lage > 8.0 { Some(lage - 9.0) } else if lage < 0.0 { Some(lage + 9.0) } else { None };
            for lage in std::iter::once(lage).chain(umlauf) {
                let x = rect.x + (lage * 20.0 - 1.0) * scale;
                let (x0, x1) = (x.max(links), (x + size).min(rechts));
                if x1 <= x0 {
                    continue;
                }
                let u = |px: f32| u0 + (u1 - u0) * (px - x) / size;
                sprites.push(UiSprite::quad(x0, y, x1 - x0, size * (23.0 / 24.0), [u(x0), v0, u(x1), v1], [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));
            }
        }
    }

    // Statusleisten ueber der Hotbar: Leben, Ruestung, Nahrung, Luft und die Erfahrungsleiste.
    //
    // Alle Masse sind Vanillas InGameHud.renderStatusBars in GUI-Einheiten, nur eben RELATIV
    // zum Hotbar-Rechteck statt absolut zum Bildschirmrand -- damit wandern die Leisten mit,
    // wenn die Hotbar im HUD-Editor verschoben wird. Die Umrechnung: Vanilla setzt die
    // Herzreihe auf `height - 39`, die Hotbar-Oberkante liegt bei `height - 22`, also 17
    // Einheiten darueber. Genauso die Erfahrungsleiste (`height - 29` = 7 darueber) und die
    // Stufenzahl (`height - 35` = 13 darueber).
    //
    // NICHT nachgebaut (bewusst, keine Luecke aus Versehen):
    //   - das Wackeln der Herzen bei wenig Leben und der Regenerations-Hupf. Beide haengen an
    //     einem Zufallsgenerator, den Vanilla mit der Tickzahl seedet; sie sind Zierrat und
    //     kosten in einem PvP-HUD nur Ruhe.
    // Sound-Debug-Modul: zwei senkrechte Pegelbalken am linken Rand, L und R. Gemessen wird
    // die Spitze des Mixer-Ausgangs (s. audio::take_peak_levels), vor der Begrenzung -- ein
    // roter Balken heisst, der Kanal uebersteuert. Die Anzeige faellt um 1,5 je Sekunde, damit
    // kurze Klaenge sichtbar bleiben.
    pub(crate) fn build_sound_meter(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        if !self.settings.module_sound_debug {
            return;
        }
        let (neu_l, neu_r) = audio::take_peak_levels();
        let (alt_l, alt_r, zuletzt) = self.sound_meter.get();
        let jetzt = Instant::now();
        let fall = zuletzt.map_or(0.0, |t| jetzt.duration_since(t).as_secs_f32() * 1.5);
        let l = neu_l.max(alt_l - fall).max(0.0);
        let r = neu_r.max(alt_r - fall).max(0.0);
        self.sound_meter.set((l, r, Some(jetzt)));

        let scale = self.hud_scale(gpu.size());
        let (_, sh) = gpu.size();
        let (w, h, abstand) = (6.0 * scale, 80.0 * scale, 3.0 * scale);
        let x0 = (8.0 * scale).round();
        let y0 = ((sh - h) / 2.0).round();
        for (i, (pegel, name)) in [(l, "L"), (r, "R")].into_iter().enumerate() {
            let x = x0 + i as f32 * (w + abstand);
            rects.push(UiRect { rect: Rect { x: x - scale, y: y0 - scale, w: w + 2.0 * scale, h: h + 2.0 * scale }, color: [0.0, 0.0, 0.0, 0.6], ..UiRect::FLAT });
            let fuellung = pegel.min(1.0) * h;
            let farbe = if pegel >= 1.0 {
                [1.0, 0.2, 0.2, 1.0]
            } else if pegel >= 0.7 {
                [1.0, 0.85, 0.2, 1.0]
            } else {
                [0.3, 0.9, 0.35, 1.0]
            };
            rects.push(UiRect { rect: Rect { x, y: y0 + h - fuellung, w, h: fuellung }, color: farbe, ..UiRect::FLAT });
            let tw = gpu.measure_text(name, scale);
            texts.push(UiText { text: name.to_string(), x: (x + (w - tw) / 2.0).round(), y: y0 + h + 3.0 * scale, color: [1.0, 1.0, 1.0, 1.0], scale });
        }
    }

    // Die eigenen Effekte aufsteigend sortiert (MobEffectInstance.compareTo). `nur_symbol`:
    // nur die mit Symbol-Flag (die HUD-Anzeige; die Liste im Inventar zeigt alle).
    pub(crate) fn effect_entries(&self, nur_symbol: bool) -> Vec<EffectEntry> {
        let now = Instant::now();
        let mut liste: Vec<EffectEntry> = self
            .shown_effects
            .iter()
            .filter(|(_, e)| !nur_symbol || e.show_icon())
            .map(|(&effect, e)| EffectEntry { effect, ambient: e.ambient(), amplifier: e.amplifier, remaining: e.remaining(now), exact: e.remaining_exact(now), received: e.duration })
            .collect();
        liste.sort_by(effect_order);
        liste
    }

    // Was die HUD-Anzeige zeigt, auf Wunsch ohne ambiente Effekte. Im Editor ohne echte
    // Effekte Beispiele (Schnelligkeit, Staerke, Vergiftung), damit es etwas zum Anfassen gibt.
    pub(crate) fn hud_effect_list(&self, beispiel: bool) -> Vec<EffectEntry> {
        let mut liste = self.effect_entries(true);
        if self.settings.effects_hide_ambient {
            liste.retain(|e| !e.ambient);
        }
        if !liste.is_empty() || !beispiel {
            return liste;
        }
        let e = |effect: i32, remaining: i32| EffectEntry { effect, ambient: false, amplifier: 0, remaining, exact: f64::from(remaining), received: remaining };
        let mut liste = vec![e(protocol::packets::MOB_EFFECT_SPEED, 3600), e(4, 2400), e(18, 900)];
        liste.sort_by(effect_order);
        liste
    }

    // Detaillierte Effektanzeige: je Effekt Icon, Name und Timer in einer Zeile. Ab dem festen
    // Rand (Settings::effects_align) untereinander -- die Timer dann als Spalte hinter dem
    // laengsten Namen -- bzw. nebeneinander. `g` = Bildschirmpixel je GUI-Pixel. Rueckgabe:
    // Plaetze relativ zur linken oberen Ecke und die Groesse der Anzeige.
    pub(crate) fn effect_detail_layout(&self, gpu: &GpuState, liste: &[EffectEntry], g: f32) -> (Vec<EffectDetailPlace>, f32, f32) {
        let set = &self.settings;
        let (symbol, nach_symbol) = if set.effects_icon { (18.0 * g, 4.0 * g) } else { (0.0, 0.0) };
        let luecke = if set.effects_name { 6.0 * g } else { 0.0 };
        let zeile = symbol.max(gpu.text_height(g));
        let breite = |t: &str| gpu.measure_text(t, g);
        // Der laengste Effekt zuerst, also am festen Rand: laeuft ein kuerzerer ab, verschwindet
        // er hinten, und die laengeren bleiben stehen, statt nachzuruecken.
        let mut liste = liste.to_vec();
        liste.sort_by(effect_longest_first);
        let mut plaetze: Vec<EffectDetailPlace> = liste
            .iter()
            .map(|e| EffectDetailPlace {
                entry: *e,
                x: 0.0,
                y: 0.0,
                h: zeile,
                name: if set.effects_name { effect_display_name(e) } else { String::new() },
                name_x: symbol + nach_symbol,
                timer: format_effect_timer(e.remaining, set.effects_timer_style),
                timer_x: 0.0,
            })
            .collect();
        let n = plaetze.len();
        if n == 0 {
            return (plaetze, 0.0, 0.0);
        }
        if matches!(set.effects_align, EFFECTS_ALIGN_LEFT | EFFECTS_ALIGN_RIGHT) {
            let abstand = 10.0 * g;
            let breiten: Vec<f32> = plaetze.iter().map(|p| symbol + nach_symbol + breite(&p.name) + luecke + breite(&p.timer)).collect();
            let gesamt = breiten.iter().sum::<f32>() + (n - 1) as f32 * abstand;
            let mut x = 0.0;
            for (p, w) in plaetze.iter_mut().zip(breiten) {
                p.x = if set.effects_align == EFFECTS_ALIGN_RIGHT { gesamt - x - w } else { x };
                p.name_x = p.x + symbol + nach_symbol;
                p.timer_x = p.name_x + breite(&p.name) + luecke;
                x += w + abstand;
            }
            return (plaetze, gesamt, zeile);
        }
        let abstand = 2.0 * g;
        let name_w = plaetze.iter().map(|p| breite(&p.name)).fold(0.0, f32::max);
        let timer_w = plaetze.iter().map(|p| breite(&p.timer)).fold(0.0, f32::max);
        for (i, p) in plaetze.iter_mut().enumerate() {
            let reihe = if set.effects_align == EFFECTS_ALIGN_BOTTOM { n - 1 - i } else { i };
            p.y = reihe as f32 * (zeile + abstand);
            p.timer_x = p.name_x + name_w + luecke;
        }
        (plaetze, symbol + nach_symbol + name_w + luecke + timer_w, n as f32 * (zeile + abstand) - abstand)
    }

    // Gui.extractEffects: je Effekt der 24x24-Rahmen und darin das 18x18-Symbol -- oder die
    // detaillierte Liste (s. effect_detail_layout). `deckkraft` blasst alles ab (Editor bei
    // ausgeschalteter Anzeige).
    pub(crate) fn build_effect_hud(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>, beispiel: bool, deckkraft: f32) {
        let liste = self.hud_effect_list(beispiel);
        if self.settings.effects_detailed {
            self.begin_text_area(gpu, texts, ShadowArea::Effects);
            let g = self.effects_scale(gpu.size());
            let rect = self.hud_effects_rect(gpu, beispiel);
            let text_h = gpu.text_height(g);
            let set = &self.settings;
            for p in self.effect_detail_layout(gpu, &liste, g).0 {
                let (x, y) = (rect.x + p.x, rect.y + p.y);
                if set.effects_icon {
                    let symbol = format!("mob_effect/{}", protocol::packets::mob_effect_name(p.entry.effect));
                    // Ohne Vanillas Blinken: das Ende zeigt hier der Timer.
                    if let Some(r) = self.ui_atlas.rect(&symbol) {
                        sprites.push(UiSprite::quad(x.round(), (y + (p.h - 18.0 * g) / 2.0).round(), 18.0 * g, 18.0 * g, r.uv, [1.0, 1.0, 1.0, deckkraft], SpriteAtlas::Ui));
                    }
                }
                let farbe = |effekt: bool| {
                    let [r, gr, b, _] = if effekt { rgb_to_rgba(world::potion::effect_color(p.entry.effect)) } else { [1.0; 4] };
                    [r, gr, b, deckkraft]
                };
                let ty = (y + (p.h - text_h) / 2.0).round();
                // Dunkle Effektfarben (Absorption, Dunkelheit, Blindheit ...) sind auf dunklem
                // Grund kaum zu lesen: dort bekommt farbiger Text einen weissen Rand statt des
                // schwarzen (s. effect_color_is_dark).
                let dunkel = effect_is_dark(p.entry.effect);
                let weisser_rand = |texts: &mut Vec<UiText>, farbig: bool| {
                    if dunkel && farbig {
                        push_span_shadow(texts, Some(0xFFFF_FFFF));
                    }
                };
                if !p.name.is_empty() {
                    let name_farbe = farbe(set.effects_name_color);
                    weisser_rand(texts, set.effects_name_color);
                    texts.push(UiText { text: p.name, x: (rect.x + p.name_x).round(), y: ty, color: name_farbe, scale: g });
                }
                let timer_farbe = if let Some(rot) = effect_timer_blink(&p.entry, set.effects_timer_blink) {
                    let [r, gr, b, _] = if rot { rgb_to_rgba(EFFECTS_BLINK_RED) } else { [1.0; 4] };
                    [r, gr, b, deckkraft]
                } else {
                    weisser_rand(texts, set.effects_timer_color);
                    farbe(set.effects_timer_color)
                };
                texts.push(UiText { text: p.timer, x: (rect.x + p.timer_x).round(), y: ty, color: timer_farbe, scale: g });
            }
            Self::end_text_area(gpu, texts);
            return;
        }
        let (plaetze, breite, _) = effect_hud_layout(&liste);
        if plaetze.is_empty() {
            return;
        }
        let rect = self.hud_effects_rect(gpu, beispiel);
        let g = self.effects_scale(gpu.size());
        let rechts = rect.x + breite * g;
        for (e, gx, gy) in plaetze {
            let (x, y) = ((rechts + gx * g).round(), (rect.y + gy * g).round());
            let rahmen = if e.ambient { "hud/effect_background_ambient" } else { "hud/effect_background" };
            if let Some(r) = self.ui_atlas.rect(rahmen) {
                sprites.push(UiSprite::quad(x, y, 24.0 * g, 24.0 * g, r.uv, [1.0, 1.0, 1.0, deckkraft], SpriteAtlas::Ui));
            }
            let symbol = format!("mob_effect/{}", protocol::packets::mob_effect_name(e.effect));
            if let Some(r) = self.ui_atlas.rect(&symbol) {
                let a = effect_icon_alpha(&e) * deckkraft;
                sprites.push(UiSprite::quad(x + 3.0 * g, y + 3.0 * g, 18.0 * g, 18.0 * g, r.uv, [1.0, 1.0, 1.0, a], SpriteAtlas::Ui));
            }
        }
    }

    // Neun-Teile-Sprite (gui.scaling nine_slice): Ecken unverzerrt, Kanten und Mitte gestreckt.
    // `rand` in Sprite-Pixeln, `g` = Bildschirmpixel je Sprite-Pixel.
    pub(crate) fn push_nine_slice(&self, sprites: &mut Vec<UiSprite>, name: &str, x: f32, y: f32, w: f32, h: f32, rand: u32, g: f32) {
        let Some(r) = self.ui_atlas.rect(name) else { return };
        let (sw, sh) = (r.width, r.height);
        let rp = rand as f32 * g;
        let spalten = [(0, rand, x, rp), (rand, sw - 2 * rand, x + rp, w - 2.0 * rp), (sw - rand, rand, x + w - rp, rp)];
        let zeilen = [(0, rand, y, rp), (rand, sh - 2 * rand, y + rp, h - 2.0 * rp), (sh - rand, rand, y + h - rp, rp)];
        for (qy, qh, zy, zh) in zeilen {
            for (qx, qw, zx, zw) in spalten {
                if zw > 0.0 && zh > 0.0 {
                    sprites.push(UiSprite::quad(zx, zy, zw, zh, r.sub(qx, qy, qw, qh), [1.0; 4], SpriteAtlas::Ui));
                }
            }
        }
    }

    // EffectsInInventory: die Liste rechts neben dem Spielerinventar. Ist genug Platz (120
    // GUI-Pixel), stehen Name und Restdauer neben dem Symbol, sonst nur das Symbol -- und der
    // Text als Hinweis unter dem Zeiger. Ab sechs Effekten ruecken die Zeilen zusammen.
    pub(crate) fn build_inventory_effects(&self, gpu: &GpuState, layout: &inventory::ScreenLayout, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let liste = self.effect_entries(false);
        let g = layout.scale;
        let x0 = layout.bg_x + layout.bg_w + 2.0 * g;
        let platz = ((gpu.size().0 - x0) / g).floor();
        if liste.is_empty() || platz < 32.0 {
            return;
        }
        let max_breite = if platz >= 120.0 { platz - 7.0 } else { 32.0 };
        let schritt = if liste.len() > 5 { (132 / (liste.len() - 1)) as f32 } else { 33.0 };
        let breite_von = |text: &str| gpu.measure_text(text, g) / g;
        let (cx, cy) = self.cursor_pos;
        let mut hinweis: Option<(String, String)> = None;
        let mut y0 = layout.bg_y;
        for e in &liste {
            let name = effect_display_name(e);
            let dauer = format_effect_duration(e.remaining);
            let breite = max_breite.min((32.0 + breite_von(&name) + 7.0).max(32.0 + breite_von(&dauer) + 7.0));
            let rahmen = if e.ambient { "container/inventory/effect_background_ambient" } else { "container/inventory/effect_background" };
            self.push_nine_slice(sprites, rahmen, x0, y0, breite * g, 32.0 * g, 4, g);
            let text_breite = breite - 32.0 - 7.0;
            let kompakt = if text_breite > 0.0 {
                let gekuerzt = breite_von(&name) > text_breite;
                let zeile = if gekuerzt { clip_with_ellipsis(&name, text_breite, &breite_von) } else { name.clone() };
                let tx = (x0 + 32.0 * g).round();
                texts.push(UiText { text: zeile, x: tx, y: (y0 + 7.0 * g).round(), color: [1.0; 4], scale: g });
                texts.push(UiText { text: dauer.clone(), x: tx, y: (y0 + 16.0 * g).round(), color: rgb_to_rgba(0x808080), scale: g });
                gekuerzt
            } else {
                true
            };
            if kompakt && cx >= x0 && cx <= x0 + breite * g && cy >= y0 && cy <= y0 + schritt * g {
                hinweis = Some((name, dauer));
            }
            let symbol = format!("mob_effect/{}", protocol::packets::mob_effect_name(e.effect));
            if let Some(r) = self.ui_atlas.rect(&symbol) {
                sprites.push(UiSprite::quad(x0 + 7.0 * g, y0 + 7.0 * g, 18.0 * g, 18.0 * g, r.uv, [1.0; 4], SpriteAtlas::Ui));
            }
            y0 += schritt * g;
        }
        // Der Hinweis mit Vanillas dunklem Tooltip-Grund. Der Grund ist ein Sprite (die
        // weisse Flaeche, getoent), weil Rechtecke VOR den Sprites gezeichnet werden und
        // sonst unter den Effektrahmen laegen.
        if let (Some((name, dauer)), Some(weiss)) = (hinweis, self.ui_atlas.rect(assets::UI_WHITE)) {
            let zeilen_h = gpu.text_height(UI_SCALE_SMALL);
            let breite = gpu.measure_text(&name, UI_SCALE_SMALL).max(gpu.measure_text(&dauer, UI_SCALE_SMALL));
            let (tx, ty) = ((cx + 12.0).round(), (cy - 12.0).round());
            sprites.push(UiSprite::quad(tx - 4.0, ty - 4.0, breite + 8.0, zeilen_h * 2.0 + 8.0, weiss.sub(1, 1, 2, 2), [0.06, 0.0, 0.06, 0.94], SpriteAtlas::Ui));
            texts.push(UiText { text: name, x: tx, y: ty, color: [1.0; 4], scale: UI_SCALE_SMALL });
            texts.push(UiText { text: dauer, x: tx, y: (ty + zeilen_h).round(), color: rgb_to_rgba(0x808080), scale: UI_SCALE_SMALL });
        }
    }

    // Saettigungs-Modul: ein Balken direkt ueber der Nahrung, so breit wie ihre zehn Symbole,
    // mit runden Enden und duennen schwarzen Linien. Zehn Felder zu je 2 Punkten, von rechts
    // gefuellt wie die Nahrung; ein ungerader Rest fuellt ein halbes Feld. Leere Felder sind
    // durchsichtig.
    pub(crate) fn build_saturation_bar(&self, gpu: &GpuState, rects: &mut Vec<UiRect>) {
        // Ziel in halben Feldern, angezeigt wird ein weich nachlaufender Wert (Halbwertszeit
        // rund 70 ms) -- der Balken gleitet statt zu springen.
        let ziel = (self.food_saturation.max(0.0).floor()).min(20.0);
        let jetzt = Instant::now();
        let gezeigt = match self.saturation_shown.get() {
            Some((alt, wann)) => {
                let dt = jetzt.duration_since(wann).as_secs_f32().min(0.5);
                let neu = alt + (ziel - alt) * (1.0 - (-dt * 10.0).exp());
                if (neu - ziel).abs() < 0.01 { ziel } else { neu }
            }
            None => ziel,
        };
        self.saturation_shown.set(Some((gezeigt, jetzt)));
        let oben = -STATUS_BARS_ABOVE_HOTBAR - self.settings.saturation_hud_height as f32 - 1.0;
        self.push_status_bar(gpu, rects, oben, gezeigt, self.settings.saturation_hud_rgb());
    }

    // Luft als Balken (Hotbar-Einstellung "Sauerstoff"): wie der Saettigungsbalken, blau, dort,
    // wo sonst die Blasen stehen -- nur, solange die Luft nicht voll ist.
    pub(crate) fn build_air_bar(&self, gpu: &GpuState, rects: &mut Vec<UiRect>) {
        if self.conn_status != ConnStatus::Connected || self.air_ticks >= MAX_AIR_TICKS {
            return;
        }
        let dicke = self.settings.saturation_hud_height as f32 + 1.0;
        let saettigung_h = if self.settings.module_saturation_hud { dicke } else { 0.0 };
        let halbe = self.air_ticks.max(0) as f32 * 20.0 / MAX_AIR_TICKS as f32;
        self.push_status_bar(gpu, rects, -STATUS_BARS_ABOVE_HOTBAR - saettigung_h - dicke, halbe, COLOR_AIR_BAR);
    }

    // Ein Balken ueber der Nahrung, so breit wie ihre zehn Symbole, mit Oberkante `oben` (GUI-
    // Punkte relativ zur Hotbar), `gezeigt` halben Feldern Fuellung von rechts und Hoehe, Grund
    // und Raster aus den Einstellungen des Saettigungsbalkens.
    pub(crate) fn push_status_bar(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, oben: f32, gezeigt: f32, rgb: [f32; 3]) {
        let rect = self.hud_hotbar_rect(gpu);
        let scale = self.hotbar_scale(gpu.size());
        let links = STATUS_BAR_WIDTH - 9.0 * STATUS_ICON_PITCH - STATUS_ICON;
        let dicke = self.settings.saturation_hud_height as f32;
        let px = |g: f32| (rect.x + g * scale).round();
        let (x0, x1) = (px(links), px(STATUS_BAR_WIDTH));
        let y0 = (rect.y + oben * scale).round();
        let h = (dicke * scale).round();
        let radius = h / 2.0;
        let linie = (scale * 0.5).round().max(1.0);
        let raster = [0.0, 0.0, 0.0, self.settings.saturation_hud_grid_alpha as f32 / 100.0];
        // Felder von rechts: Feld j reicht von grenze(j + 1) bis grenze(j).
        let grenze = |j: f32| x1 - (x1 - x0) * j / 10.0;
        // Grund und Fuellung liegen INNERHALB des Rahmens: um die Linienbreite eingerueckt, mit
        // dem entsprechend kleineren Radius -- sonst schauen sie an den runden Enden heraus.
        let innen = Rect { x: x0 + linie, y: y0 + linie, w: x1 - x0 - 2.0 * linie, h: h - 2.0 * linie };
        let innen_radius = (radius - linie).max(0.0);
        if self.settings.saturation_hud_bg_alpha > 0 {
            rects.push(UiRect::rounded(innen, [0.0, 0.0, 0.0, self.settings.saturation_hud_bg_alpha as f32 / 100.0], innen_radius));
        }
        if gezeigt > 0.0 {
            let fuell_links = grenze(gezeigt / 2.0).max(innen.x);
            let fuellung = Rect { x: fuell_links, w: innen.x + innen.w - fuell_links, ..innen };
            rects.push(UiRect::rounded(fuellung, [rgb[0], rgb[1], rgb[2], 1.0], innen_radius.min(fuellung.w / 2.0)));
        }
        if raster[3] > 0.0 {
            for j in 1..10 {
                let x = grenze(j as f32).round();
                rects.push(UiRect::rounded(Rect { x: x - linie / 2.0, y: y0 + linie, w: linie, h: h - 2.0 * linie }, raster, 0.0));
            }
            rects.push(UiRect::framed(Rect { x: x0, y: y0, w: x1 - x0, h }, [0.0; 4], radius, linie, raster));
        }
    }
}
