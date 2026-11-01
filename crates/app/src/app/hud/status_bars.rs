// Statusleisten, Bossleisten und Erfahrungsleiste.

use super::*;

impl App {
    pub(crate) fn build_status_bars(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        if self.conn_status != ConnStatus::Connected {
            return;
        }
        let rect = self.hud_hotbar_rect(gpu);
        let scale = self.hotbar_scale(gpu.size());
        // Ein 9x9-Symbol an eine GUI-Koordinate setzen, mit Deckkraft.
        let icon_alpha = |sprites: &mut Vec<UiSprite>, name: &str, gx: f32, gy: f32, alpha: f32| {
            if let Some(sprite) = self.ui_atlas.rect(name) {
                sprites.push(UiSprite::quad(
                    (rect.x + gx * scale).round(),
                    (rect.y + gy * scale).round(),
                    STATUS_ICON * scale,
                    STATUS_ICON * scale,
                    sprite.uv,
                    [1.0, 1.0, 1.0, alpha],
                    SpriteAtlas::Ui,
                ));
            }
        };
        let icon = |sprites: &mut Vec<UiSprite>, name: &str, gx: f32, gy: f32| icon_alpha(sprites, name, gx, gy, 1.0);
        let jetzt = Instant::now();
        // Herzart wie Gui.HeartType.forPlayer: Gift vor Wither vor Frost (komplett eingefroren
        // = Entity.isFullyFrozen, 140 Ticks).
        let art = if self.shown_effects.contains_key(&protocol::packets::MOB_EFFECT_POISON) {
            "poisoned_"
        } else if self.shown_effects.contains_key(&protocol::packets::MOB_EFFECT_WITHER) {
            "withered_"
        } else if self.own_frozen_ticks >= 140 {
            "frozen_"
        } else {
            ""
        };
        let herz = |name: &str| heart_variant(name, art, self.hardcore);
        let behaelter = herz("hud/heart/container");

        // Zeilenaufteilung der Herzen: mehr als 10 Herzen stapeln sich, und je mehr Zeilen es
        // werden, desto enger ruecken sie zusammen (Vanilla: max(10 - (rows - 2), 3)).
        // Vanilla rundet AUF (InGameHud.renderStatusBars: MathHelper.ceil(getHealth())) --
        // abgerundet fehlte bei krummen Werten (UHC ohne Regeneration: 19.3) ein halbes Herz.
        let health = self.health.min(self.max_health).ceil();
        let hearts = ((self.max_health / 2.0).ceil() as i32).max(1);
        // Absorption (InGameHud.renderHealthBar): eigene goldene Herzen HINTER den normalen,
        // mit leerem Behaelter darunter -- und sie zaehlen bei der Zeilenzahl mit, damit
        // ruecken auch Herzzeilen und Ruestung zusammen bzw. hoch.
        let absorption = self.absorption.ceil() as i32;
        let absorption_hearts = (absorption as f32 / 2.0).ceil() as i32;
        let rows = ((f32::midpoint(self.max_health, absorption as f32) / 10.0).ceil() as i32).max(1);
        let line_height = (10 - (rows - 2)).max(3) as f32;
        let heart_row_y = -STATUS_BARS_ABOVE_HOTBAR;

        // Seit wann jedes Absorptionsherz da ist: verschwundene vergessen, neue ab jetzt.
        let seit = {
            let mut seit = self.absorption_since.borrow_mut();
            seit.truncate(absorption_hearts as usize);
            seit.resize(absorption_hearts as usize, jetzt);
            seit.clone()
        };

        // Von hinten nach vorn, damit die obere Zeile unter der unteren liegt -- genau wie
        // Vanillas absteigende Schleife.
        for l in (0..hearts + absorption_hearts).rev() {
            let gx = (l % 10) as f32 * STATUS_ICON_PITCH;
            let gy = heart_row_y - (l / 10) as f32 * line_height;
            if l >= hearts {
                // Neue Absorptionsherzen blenden ein und gleiten dabei von unten an ihren Platz.
                let t = (jetzt.duration_since(seit[(l - hearts) as usize]).as_secs_f32() / ABSORPTION_FADE_SECS).min(1.0);
                let t = 1.0 - (1.0 - t) * (1.0 - t);
                let gy = gy + (1.0 - t) * ABSORPTION_FADE_RISE;
                let sprite = absorbing_heart_sprite(absorption, l - hearts);
                if sprite == Some("hud/heart/absorbing_half") {
                    // Beim halben nur die linke Haelfte des Behaelters: rechts umrandet er
                    // kein Herz.
                    if let Some(behaelter) = self.ui_atlas.rect(&behaelter) {
                        sprites.push(UiSprite::quad(
                            (rect.x + gx * scale).round(),
                            (rect.y + gy * scale).round(),
                            5.0 * scale,
                            STATUS_ICON * scale,
                            behaelter.sub(0, 0, 5, 9),
                            [1.0, 1.0, 1.0, t],
                            SpriteAtlas::Ui,
                        ));
                    }
                } else {
                    icon_alpha(sprites, &behaelter, gx, gy, t);
                }
                if let Some(sprite) = sprite {
                    icon_alpha(sprites, &herz(sprite), gx, gy, t);
                }
            } else {
                icon(sprites, &behaelter, gx, gy);
                if let Some(sprite) = heart_sprite(health, l) {
                    icon(sprites, &herz(sprite), gx, gy);
                }
            }
        }

        // Ruestung: eine Zeile ueber der obersten Herzzeile. Ohne Ruestung zeichnet Vanilla
        // gar nichts -- auch keine leeren Symbole. Aendert sich die Zahl der Herzzeilen, gleitet
        // sie zum neuen Platz (wie der Saettigungsbalken, s. build_saturation_bar).
        let ziel = heart_row_y - (rows - 1) as f32 * line_height - STATUS_ROW_PITCH;
        let armor_y = match self.armor_row_shown.get() {
            Some((alt, wann)) => {
                let dt = jetzt.duration_since(wann).as_secs_f32().min(0.5);
                let neu = alt + (ziel - alt) * (1.0 - (-dt * 12.0).exp());
                if (neu - ziel).abs() < 0.01 { ziel } else { neu }
            }
            None => ziel,
        };
        self.armor_row_shown.set(Some((armor_y, jetzt)));
        if self.armor > 0 {
            let gy = armor_y;
            for l in 0..10 {
                let gx = l as f32 * STATUS_ICON_PITCH;
                icon(sprites, armor_sprite(self.armor, l), gx, gy);
            }
        }

        // Nahrung: rechte Seite, von rechts nach links aufgefuellt.
        for j in 0..10 {
            let gx = STATUS_BAR_WIDTH - j as f32 * STATUS_ICON_PITCH - STATUS_ICON;
            icon(sprites, "hud/food_empty", gx, heart_row_y);
            if let Some(sprite) = food_sprite(self.food_level, j) {
                icon(sprites, sprite, gx, heart_row_y);
            }
        }

        // Platz fuer den Saettigungsbalken (s. build_saturation_bar): die Luft rueckt darueber.
        let saettigung_h = if self.settings.module_saturation_hud { self.settings.saturation_hud_height as f32 + 1.0 } else { 0.0 };

        // Luft: nur wenn sie nicht voll ist -- ueber der Nahrungsleiste. Als Balken zeichnet
        // sie build_air_bar. Vanilla blendet sie
        // ausserdem beim Auftauchen noch kurz ein, solange sie sich wieder fuellt; genau das
        // ergibt sich hier von selbst, weil der Server den steigenden Wert mitschickt.
        if self.air_ticks < MAX_AIR_TICKS && !self.settings.hotbar_air_bar {
            // Genau EINE Zeile ueber der Nahrung. In Vanilla steht das verschraenkt da: erst
            // `r = n - 10`, dann wegen der gezeichneten Nahrungsleiste noch einmal `r -= 10`,
            // und getAirBubbleY() addiert ueber `getHeartRows(0) - 1 = -1` zehn wieder drauf.
            // Unterm Strich bleibt eine Zeile -- die beiden letzten Schritte heben sich auf,
            // solange man auf keinem Reittier sitzt (Reittiere fuehrt der Client nicht).
            let gy = heart_row_y - STATUS_ROW_PITCH - saettigung_h;
            let voll = air_bubbles(self.air_ticks);
            for n in 1..=10 {
                let gx = STATUS_BAR_WIDTH - (n - 1) as f32 * STATUS_ICON_PITCH - STATUS_ICON;
                if n <= voll {
                    icon(sprites, "hud/air", gx, gy);
                } else if n == voll + 1 {
                    // Die eine Blase, die gerade platzt.
                    icon(sprites, "hud/air_bursting", gx, gy);
                }
            }
        }

        self.build_experience_bar(gpu, rect, scale, sprites, texts);
    }

    // Bossbars (BossBarHud.render): je Bar 182x5, die erste 12 unter der Oberkante, der Name 9
    // darueber, die naechste 19 tiefer; Schluss, sobald ein Drittel der Bildhoehe
    // durchlaufen ist. Die Lage kommt aus dem HUD-Editor (s. hud_bossbar_rect) -- im Standard
    // genau Vanillas oben mittig.
    pub(crate) fn build_boss_bars(&self, gpu: &GpuState, bars: &[BossBarState], sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        if bars.is_empty() {
            return;
        }
        const BREITE: f32 = 182.0;
        const HOEHE: f32 = 5.0;
        let (sw, sh) = gpu.size();
        let scale = self.bossbar_scale((sw, sh));
        let rect = self.hud_bossbar_rect(gpu);
        let x = rect.x;
        // Der Name mittig ueber der Bar -- im Standard Vanillas Bildschirmmitte, die von der
        // Bar-Mitte um bis zu einen GUI-Pixel abweicht (die Bar steht auf ganzen GUI-Pixeln).
        let mitte = (self.settings.hud_bossbar_x * sw).clamp(rect.x + rect.w / 2.0 - scale, rect.x + rect.w / 2.0 + scale);
        let mut y = 12.0f32;
        let jetzt = Instant::now();
        self.begin_text_area(gpu, texts, ShadowArea::Title);
        for bar in bars {
            let sy = (rect.y + y * scale).round();
            let lage = |sprites: &mut Vec<UiSprite>, name: &str, w: f32| {
                if let Some(r) = self.ui_atlas.rect(name) {
                    sprites.push(UiSprite::quad(x.round(), sy, w * scale, HOEHE * scale, r.sub(0, 0, w as u32, HOEHE as u32), [1.0; 4], SpriteAtlas::Ui));
                }
            };
            let farbe = BOSS_BAR_COLORS.get(bar.color as usize).copied().unwrap_or("white");
            let kerben = (bar.style >= 1).then(|| BOSS_BAR_NOTCHES.get(bar.style as usize - 1).copied()).flatten();
            lage(sprites, &format!("boss_bar/{farbe}_background"), BREITE);
            if let Some(k) = kerben {
                lage(sprites, &format!("boss_bar/notched_{k}_background"), BREITE);
            }
            // MathHelper.lerpPositive(percent, 0, 182): abgerundet, mindestens 0.
            let gefuellt = (bar.percent(jetzt) * BREITE).floor().clamp(0.0, BREITE);
            if gefuellt > 0.0 {
                lage(sprites, &format!("boss_bar/{farbe}_progress"), gefuellt);
                if let Some(k) = kerben {
                    lage(sprites, &format!("boss_bar/notched_{k}_progress"), gefuellt);
                }
            }
            // Auf Viertel gerundet wie title_scale (bei ganzer GUI-Skala unveraendert).
            let text_scale = ((scale * 4.0).round() / 4.0).max(1.0);
            let name = self.ego_spans(&bar.name);
            let w = span_row_width(gpu, &name, text_scale);
            let tx = (mitte - w / 2.0).round();
            let ty = (rect.y + (y - 9.0) * scale).round();
            let weiss = self.ui_atlas.rect(assets::UI_WHITE).map(|r| r.uv);
            push_styled_row(gpu, &name, tx, ty, COLOR_WHITE, text_scale, texts, &mut |r, c| {
                if let Some(uv) = weiss {
                    sprites.push(UiSprite::quad(r.x, r.y, r.w, r.h, uv, c, SpriteAtlas::Ui));
                }
            });
            y += 19.0;
            if y * scale >= sh / 3.0 {
                break;
            }
        }
        Self::end_text_area(gpu, texts);
    }

    // Wie viele Bossbars build_boss_bars zeichnet (mindestens eine, fuer den Platzhalter).
    pub(crate) fn boss_bars_drawn(&self, scale: f32, sh: f32) -> usize {
        let mut n = 0;
        let mut y = 12.0f32;
        for _ in 0..self.boss_bars.len().max(1) {
            n += 1;
            y += 19.0;
            if y * scale >= sh / 3.0 {
                break;
            }
        }
        n
    }

    // Griff-/Zeichen-Rechteck der Bossbars: 182 GUI-Pixel breit, vom ersten Namen (3 unter der
    // Oberkante, darueber frei) bis unter die letzte Bar. hud_bossbar_x ist die Mitte, auf
    // ganze GUI-Pixel wie Vanillas `scaledWidth / 2 - 91`.
    pub(crate) fn hud_bossbar_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let scale = self.bossbar_scale((sw, sh));
        let w = 182.0 * scale;
        let h = (19.0 * self.boss_bars_drawn(scale, sh) as f32 - 2.0) * scale;
        let x = (((self.settings.hud_bossbar_x * sw / scale).floor() - 91.0) * scale).clamp(0.0, (sw - w).max(0.0)).round();
        let y = (self.settings.hud_bossbar_y * sh).clamp(0.0, (sh - h).max(0.0)).round();
        Rect { x, y, w, h }
    }

    // BossBarS2CPacket auf die Liste anwenden (BossBarHud.handlePacket). Aenderungen fuer
    // eine unbekannte Bar laufen ins Leere.
    pub(crate) fn on_boss_event(&mut self, uuid: [u8; 16], event: protocol::packets::BossEvent) {
        use protocol::packets::BossEvent as E;
        let jetzt = Instant::now();
        if let E::Add { name, progress, color, style, .. } = event {
            let neu = BossBarState { uuid, name, shown: progress, target: progress, set_at: jetzt, color, style };
            match self.boss_bars.iter_mut().find(|b| b.uuid == uuid) {
                Some(b) => *b = neu,
                None => self.boss_bars.push(neu),
            }
            return;
        }
        if event == E::Remove {
            self.boss_bars.retain(|b| b.uuid != uuid);
            return;
        }
        let Some(bar) = self.boss_bars.iter_mut().find(|b| b.uuid == uuid) else { return };
        match event {
            E::Progress(p) => {
                // ClientBossBar.setPercent: vom gerade angezeigten Stand aus 100 ms zum neuen.
                bar.shown = bar.percent(jetzt);
                bar.target = p;
                bar.set_at = jetzt;
            }
            E::Name(name) => bar.name = name,
            E::Style { color, style } => {
                bar.color = color;
                bar.style = style;
            }
            E::Flags(_) | E::Add { .. } | E::Remove => {}
        }
    }

    // Erfahrungsleiste samt Stufenzahl (Vanilla: hud.bar.ExperienceBar + Bar.drawExperienceLevel).
    pub(crate) fn build_experience_bar(&self, gpu: &GpuState, rect: Rect, scale: f32, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let Some(background) = self.ui_atlas.rect("hud/experience_bar_background") else { return };
        let y = (rect.y - XP_BAR_ABOVE_HOTBAR * scale).round();
        sprites.push(UiSprite::quad(
            rect.x,
            y,
            STATUS_BAR_WIDTH * scale,
            XP_BAR_HEIGHT * scale,
            background.uv,
            [1.0, 1.0, 1.0, 1.0],
            SpriteAtlas::Ui,
        ));
        // Vanilla rechnet mit 183 (nicht 182) und schneidet den Ueberhang ab -- so ist die
        // Leiste bei Fortschritt 1.0 sicher ganz voll und nicht einen Pixel zu kurz.
        let gefuellt = (self.xp_progress * (STATUS_BAR_WIDTH + 1.0)).floor().min(STATUS_BAR_WIDTH);
        if gefuellt >= 1.0 {
            if let Some(progress) = self.ui_atlas.rect("hud/experience_bar_progress") {
                sprites.push(UiSprite::quad(
                    rect.x,
                    y,
                    gefuellt * scale,
                    XP_BAR_HEIGHT * scale,
                    progress.sub(0, 0, gefuellt as u32, XP_BAR_HEIGHT as u32),
                    [1.0, 1.0, 1.0, 1.0],
                    SpriteAtlas::Ui,
                ));
            }
        }

        if self.xp_level > 0 {
            let text = self.xp_level.to_string();
            let text_scale = scale.round().max(1.0);
            let width = gpu.measure_text(&text, text_scale);
            let x = (rect.x + (STATUS_BAR_WIDTH * scale - width) / 2.0).round();
            let ty = (rect.y - XP_LEVEL_ABOVE_HOTBAR * scale).round();
            // Vanilla zeichnet die Zahl vier Mal schwarz um einen Pixel versetzt und einmal
            // gruen obendrauf -- ein Umriss, damit sie auf jedem Untergrund lesbar bleibt.
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                texts.push(UiText {
                    text: text.clone(),
                    x: x + dx * scale,
                    y: ty + dy * scale,
                    color: [0.0, 0.0, 0.0, 1.0],
                    scale: text_scale,
                });
            }
            texts.push(UiText { text, x, y: ty, color: COLOR_XP_LEVEL, scale: text_scale });
        }
    }
}
