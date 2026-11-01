// Spielerliste und Server-HUD.

use super::*;

impl App {
    // Die gelisteten Eintraege in Vanillas Reihenfolge (PlayerListHud.ENTRY_ORDERING):
    // hoehere list_order zuerst, Zuschauer ans Ende, dann nach Teamname und zuletzt nach
    // Spielername ohne Gross-/Kleinschreibung. Hoechstens 80.
    pub(crate) fn player_list_entries(&self) -> Vec<(&[u8; 16], &TabEntry)> {
        let mut liste: Vec<(&[u8; 16], &TabEntry)> = self.tab_entries.iter().filter(|(_, e)| e.listed).collect();
        liste.sort_by(|(_, a), (_, b)| {
            let team = |e: &TabEntry| self.sb_entry_team.get(&e.name).cloned().unwrap_or_default();
            b.list_order
                .cmp(&a.list_order)
                .then_with(|| (a.game_mode == i32::from(GAME_MODE_SPECTATOR)).cmp(&(b.game_mode == i32::from(GAME_MODE_SPECTATOR))))
                .then_with(|| team(a).cmp(&team(b)))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        liste.truncate(PLAYER_LIST_MAX_ENTRIES);
        liste
    }

    // PlayerListHud.getPlayerName: der Anzeigename des Servers, sonst der Name im Kleid
    // seines Teams (Team.decorateName -- Prefix, Name, Suffix, alles in der Teamfarbe).
    pub(crate) fn player_list_name(&self, entry: &TabEntry) -> Vec<protocol::nbt::TextSpan> {
        let spans = if let Some(name) = &entry.display_name { name.clone() } else {
            let farbe = self.sb_entry_team.get(&entry.name).and_then(|t| self.sb_teams.get(t)).and_then(|t| protocol::nbt::formatting_color(t.color));
            protocol::nbt::legacy_to_spans(&self.decorate_team_name(&entry.name), farbe)
        };
        self.ego_rewrite(&spans).unwrap_or(spans)
    }

    // ReadableScoreboardScore.getFormattedScore: das Zahlenformat des Scores, sonst das des
    // Objectives, sonst Vanillas Vorgabe (gelbe Zahl). Ohne Score gar nichts.
    pub(crate) fn player_list_score_text(&self, objective: &str, holder: &str) -> Vec<protocol::nbt::TextSpan> {
        use protocol::packets::NumberFormat;
        let Some((score, _)) = self.sb_scores.get(objective).and_then(|m| m.get(holder)) else { return Vec::new() };
        let schluessel = (objective.to_string(), holder.to_string());
        let format = self.sb_score_formats.get(&schluessel).or_else(|| self.sb_objective_formats.get(objective));
        match format {
            Some(NumberFormat::Blank) => Vec::new(),
            Some(NumberFormat::Fixed(text)) => text.clone(),
            Some(NumberFormat::Styled(farbe)) => {
                vec![protocol::nbt::TextSpan { text: score.to_string(), color: *farbe, ..Default::default() }]
            }
            None => vec![protocol::nbt::TextSpan { text: score.to_string(), color: Some(PLAYER_LIST_SCORE_COLOR), ..Default::default() }],
        }
    }

    // Atlas-Rechteck eines 8x8-Ausschnitts aus dem Skin (Gesicht bzw. Hutlage).
    // Ein Gesicht (8x8 ab px/py) aus dem geladenen Skin. Ist er (noch) nicht im Atlas, steht
    // schwarz-lila da statt eines Standard-Gesichts -- das saehe aus wie ein echter Skin.
    pub(crate) fn skin_face_uv(&self, uuid: &[u8; 16], px: f32, py: f32) -> [f32; 4] {
        let [u0, v0, u1, v1] = self
            .skins
            .get(uuid)
            .and_then(|s| s.slot)
            .and_then(|slot| self.entity_atlas.skin_slot_rect(slot))
            .or_else(|| self.entity_atlas.rect(assets::MISSING_SKIN))
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let (du, dv) = ((u1 - u0) / 64.0, (v1 - v0) / 64.0);
        [u0 + px * du, v0 + py * dv, u0 + (px + 8.0) * du, v0 + (py + 8.0) * dv]
    }

    // Vanillas PlayerListHud.render, Mass fuer Mass in GUI-Einheiten (mal `g`, dem
    // Hotbar-Massstab). Bewusste Abweichungen:
    //   - Koepfe gibt es immer, nicht nur bei verschluesselter Verbindung: Vanilla laesst sie
    //     auf Offline-Servern weg, weil es dort keine Skins gibt -- der schwarz-lila Kopf
    //     stoert aber nicht und haelt die Spalten gleich breit.
    //   - Kein Blinken der Herzen bei Aenderung und keine kursive Schrift fuer Zuschauer
    //     (die Schrift hat keinen Kursivschnitt); Zuschauer sind wie in Vanilla blasser.
    pub(crate) fn build_player_list(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let (sw, sh) = gpu.size();
        let g = self.player_list_scale(gpu.size());
        let breite = |spans: &[protocol::nbt::TextSpan]| -> f32 { spans.iter().map(|s| gpu.measure_text(&s.text, g)).sum() };
        let schreibe = |texts: &mut Vec<UiText>, spans: &[protocol::nbt::TextSpan], x: f32, y: f32, alpha: f32| {
            let mut x = x.round();
            for span in spans {
                let mut farbe = span.color.map_or(COLOR_WHITE, rgb_to_rgba);
                farbe[3] *= alpha;
                push_span_shadow(texts, span.shadow);
                texts.push(UiText { text: span.text.clone(), x, y: y.round(), color: farbe, scale: g });
                x += gpu.measure_text(&span.text, g);
            }
        };
        let flaeche = |rects: &mut Vec<UiRect>, x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4]| {
            let (x0, y0, x1, y1) = (x0.round(), y0.round(), x1.round(), y1.round());
            rects.push(UiRect { rect: Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, color, ..UiRect::FLAT });
        };
        let symbol = |sprites: &mut Vec<UiSprite>, name: &str, x: f32, y: f32, w: f32, h: f32| {
            if let Some(r) = self.ui_atlas.rect(name) {
                sprites.push(UiSprite::quad(x.round(), y.round(), w * g, h * g, r.uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));
            }
        };

        let liste = self.player_list_entries();
        let objective = self.sb_list_objective.as_deref().filter(|o| self.sb_objective_titles.contains_key(*o));
        let herzen = objective.is_some_and(|o| self.sb_objective_hearts.contains(o));
        let leer = gpu.measure_text(" ", g);

        // Namen und Scores vorbereiten, dabei die breitesten merken (j und k in Vanilla).
        let mut max_name = 0.0f32;
        let mut max_score = 0.0f32;
        let mut zeilen: Vec<(Vec<protocol::nbt::TextSpan>, Vec<protocol::nbt::TextSpan>, i32)> = Vec::with_capacity(liste.len());
        for (_, entry) in &liste {
            let name = self.player_list_name(entry);
            max_name = max_name.max(breite(&name));
            let (mut score_text, mut score) = (Vec::new(), 0);
            if let Some(obj) = objective {
                score = self.sb_scores.get(obj).and_then(|m| m.get(&entry.name)).map_or(0, |(s, _)| *s);
                if !herzen {
                    score_text = self.player_list_score_text(obj, &entry.name);
                    let w = breite(&score_text);
                    max_score = max_score.max(if w > 0.0 { leer + w } else { 0.0 });
                }
            }
            zeilen.push((name, score_text, score));
        }

        // Spalten: so viele, dass keine laenger als 20 Zeilen wird.
        let n = liste.len();
        let mut spalten = 1usize;
        let mut reihen = n;
        while reihen > PLAYER_LIST_MAX_ROWS {
            spalten += 1;
            reihen = n.div_ceil(spalten);
        }
        let score_breite = match objective {
            Some(_) if herzen => PLAYER_LIST_HEARTS_WIDTH * g,
            Some(_) => max_score,
            None => 0.0,
        };
        let p = spalten as f32;
        let spalte = ((p * (9.0 * g + max_name + score_breite + 13.0 * g)).min(sw - 50.0 * g) / p).floor();
        // Fest oben mittig wie in Vanilla: Text bei 10 * g, der Hintergrund beginnt g darueber.
        let _ = sh;
        let mut y = (10.0 * g).round();
        let mut t = spalte * p + (p - 1.0) * 5.0 * g;
        let umbrechen = |spans: &[protocol::nbt::TextSpan]| -> Vec<Vec<protocol::nbt::TextSpan>> {
            if spans.is_empty() {
                return Vec::new();
            }
            wrap_label_lines(&split_lines_vanilla(spans), sw - 50.0 * g, |text| gpu.measure_text(text, g))
        };
        let kopf = umbrechen(&self.tab_header);
        let fuss = umbrechen(&self.tab_footer);
        for zeile in kopf.iter().chain(fuss.iter()) {
            t = t.max(breite(zeile));
        }
        let zeile_h = 9.0 * g;
        let mitte = (sw / 2.0).round();
        let links = mitte - f32::midpoint(spalte * p, (p - 1.0) * 5.0 * g);

        if !kopf.is_empty() {
            flaeche(rects, mitte - t / 2.0 - g, y - g, mitte + t / 2.0 + g, y + kopf.len() as f32 * zeile_h, black_veil(PLAYER_LIST_BG_ALPHA));
            for zeile in &kopf {
                schreibe(texts, zeile, mitte - breite(zeile) / 2.0, y, 1.0);
                y += zeile_h;
            }
            y += g;
        }

        flaeche(rects, mitte - t / 2.0 - g, y - g, mitte + t / 2.0 + g, y + reihen as f32 * zeile_h, black_veil(PLAYER_LIST_BG_ALPHA));
        for (w, ((uuid, entry), (name, score_text, score))) in liste.iter().zip(&zeilen).enumerate() {
            let (u, x) = (w / reihen.max(1), w % reihen.max(1));
            let spalte_x = links + u as f32 * (spalte + 5.0 * g);
            let z = y + x as f32 * zeile_h;
            flaeche(rects, spalte_x, z, spalte_x + spalte, z + 8.0 * g, white_veil(PLAYER_LIST_ROW_ALPHA));
            let zuschauer = entry.game_mode == i32::from(GAME_MODE_SPECTATOR);

            // Kopf: Gesicht, darueber die Hutlage (sofern der Spieler sie zeigt). Bevorzugt
            // aus dem eigenen Kopf-Platz (s. store_tab_head), sonst aus dem Skin-Platz.
            let eigener = self.tab_heads.get(*uuid).and_then(|(slot, _)| self.entity_atlas.head_slot_rect(*slot));
            let (kopf_uv, hut_uv) = match eigener {
                Some([u0, v0, u1, v1]) => {
                    let mitte = f32::midpoint(u0, u1);
                    ([u0, v0, mitte, v1], [mitte, v0, u1, v1])
                }
                None => (self.skin_face_uv(uuid, 8.0, 8.0), self.skin_face_uv(uuid, 40.0, 8.0)),
            };
            sprites.push(UiSprite::quad(spalte_x.round(), z.round(), 8.0 * g, 8.0 * g, kopf_uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Entity));
            if entry.show_hat {
                sprites.push(UiSprite::quad(spalte_x.round(), z.round(), 8.0 * g, 8.0 * g, hut_uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Entity));
            }
            let text_x = spalte_x + 9.0 * g;
            // Die Deckkraft wie bei white_veil umgerechnet: weisse Schrift ueber dunklem Grund.
            schreibe(texts, name, text_x, z, if zuschauer { world::sky::srgb_to_linear(PLAYER_LIST_SPECTATOR_ALPHA) } else { 1.0 });

            if objective.is_some() && !zuschauer {
                let aa = text_x + max_name + g;
                let ab = aa + score_breite;
                if ab - aa > 5.0 * g {
                    if herzen {
                        self.build_player_list_hearts(gpu, texts, sprites, *score, aa, ab, z, g);
                    } else if !score_text.is_empty() {
                        schreibe(texts, score_text, ab - breite(score_text), z, 1.0);
                    }
                }
            }

            // Ping-Balken (PlayerListHud.renderLatencyIcon).
            let ping = match entry.latency {
                l if l < 0 => "icon/ping_unknown",
                l if l < 150 => "icon/ping_5",
                l if l < 300 => "icon/ping_4",
                l if l < 600 => "icon/ping_3",
                l if l < 1000 => "icon/ping_2",
                _ => "icon/ping_1",
            };
            symbol(sprites, ping, spalte_x + spalte - 11.0 * g, z, 10.0, 8.0);
        }

        if !fuss.is_empty() {
            y += reihen as f32 * zeile_h + g;
            flaeche(rects, mitte - t / 2.0 - g, y - g, mitte + t / 2.0 + g, y + fuss.len() as f32 * zeile_h, black_veil(PLAYER_LIST_BG_ALPHA));
            for zeile in &fuss {
                schreibe(texts, zeile, mitte - breite(zeile) / 2.0, y, 1.0);
                y += zeile_h;
            }
        }
    }

    // PlayerListHud.renderHearts ohne das Blinken: Herzcontainer bis mindestens 10, darin
    // die vollen/halben Herzen des Scores (ab dem elften goldene). Ist fuer Herzen kein Platz
    // (Abstand <= 3 Einheiten), steht stattdessen "<Leben>hp" in einer Farbe von Rot nach Gruen.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_player_list_hearts(
        &self,
        gpu: &GpuState,
        texts: &mut Vec<UiText>,
        sprites: &mut Vec<UiSprite>,
        score: i32,
        links: f32,
        rechts: f32,
        y: f32,
        g: f32,
    ) {
        let volle = (score.max(0) + 1) / 2;
        let container = score.max(20) / 2;
        if volle <= 0 {
            return;
        }
        let abstand = (((rechts - links) / g - 4.0) / container as f32).min(9.0).floor();
        if abstand <= 3.0 {
            let anteil = (score as f32 / 20.0).clamp(0.0, 1.0);
            let rgb = (((1.0 - anteil) * 255.0) as u32) << 16 | ((anteil * 255.0) as u32) << 8;
            let leben = score as f32 / 2.0;
            let mut text = format!("{leben:?}hp");
            if rechts - gpu.measure_text(&text, g) < links {
                text = format!("{leben:?}");
            }
            let w = gpu.measure_text(&text, g);
            texts.push(UiText { text, x: ((rechts + links - w) / 2.0).round(), y: y.round(), color: rgb_to_rgba(rgb), scale: g });
            return;
        }
        let mut herz = |name: &str, l: i32| {
            if let Some(r) = self.ui_atlas.rect(name) {
                let x = (links + l as f32 * abstand * g).round();
                sprites.push(UiSprite::quad(x, y.round(), 9.0 * g, 9.0 * g, r.uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));
            }
        };
        for l in volle..container {
            herz("hud/heart/container", l);
        }
        for l in 0..volle {
            herz("hud/heart/container", l);
            if l * 2 + 1 < score {
                herz(if l >= 10 { "hud/heart/absorbing_full" } else { "hud/heart/full" }, l);
            }
            if l * 2 + 1 == score {
                herz(if l >= 10 { "hud/heart/absorbing_half" } else { "hud/heart/half" }, l);
            }
        }
    }

    // Scoreboard-Seitenleiste (rechts), Titel/Untertitel (Mitte) und Actionbar (ueber der
    // Hotbar) -- Serverinhalt, immer sichtbar im World-Modus (wie der Chat, nicht an show_hud
    // gekoppelt). Farben/Formatierung der Text-Components sind (noch) auf Weiss reduziert.
    pub(crate) fn build_server_hud(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {

        // --- Scoreboard-Seitenleiste (Position aus dem HUD-Editor, s. hud_scoreboard_rect) ---
        if let Some((title, lines)) = self.scoreboard_lines_shown() {
            self.begin_text_area(gpu, texts, ShadowArea::Scoreboard);
            let rect = self.hud_scoreboard_rect(gpu);
            let (scale, line_h, title_h, pad) = self.scoreboard_metrics(gpu);
            rects.push(UiRect { rect, color: COLOR_SCOREBOARD_BG, ..UiRect::FLAT });
            // Ein UiText je Abschnitt (eine Zeile kennt nur eine Farbe) -- dieselbe
            // Aufteilung wie im Chat.
            // Jede Zeile als GANZES auf wechselnde Zahlen pruefen (s. stable_numbers): die
            // Zahl steht meist in einem eigenen Farbabschnitt, und der allein verraet nicht,
            // was hinter ihm rutschen wuerde.
            let voll = |spans: &[protocol::nbt::TextSpan]| spans.iter().map(|s| s.text.as_str()).collect::<String>();
            gpu.observe_numbers(std::iter::once(voll(&title)).chain(lines.iter().map(|z| voll(z))).collect::<Vec<_>>().iter().map(std::string::String::as_str));
            let tw = stable_row_width(gpu, &title, scale);
            let tx = (rect.x + (rect.w - tw) / 2.0).round();
            push_stable_spans(gpu, &title, tx, (rect.y + (title_h - gpu.text_height(scale)) / 2.0).round(), COLOR_HUD, scale, texts, rects);
            let mut y = rect.y + title_h;
            for zeile in lines {
                push_stable_spans(gpu, &zeile, (rect.x + pad).round(), y.round(), COLOR_WHITE, scale, texts, rects);
                y += line_h;
            }
            Self::end_text_area(gpu, texts);
        }

        // --- Titel / Untertitel: jeder an seiner eigenen Stelle (s. HUD-Editor) ---
        let total_secs = (self.title_fade_in + self.title_stay + self.title_fade_out).max(0) as f32 / 20.0;
        if (self.title_text.is_some() || self.subtitle_text.is_some()) && self.title_set_at.elapsed().as_secs_f32() <= total_secs {
            self.begin_text_area(gpu, texts, ShadowArea::Title);
            // Ein UiText je Farbabschnitt, nebeneinander, in der Ausrichtung der Zeile.
            for (spans, subtitle) in [(&self.title_text, false), (&self.subtitle_text, true)] {
                let Some(spans) = spans else { continue };
                let block = self.hud_title_line_rect(gpu, subtitle);
                let a = if subtitle { self.settings.hud_subtitle_align } else { self.settings.hud_title_align };
                let scale = self.title_scale(subtitle);
                let w = span_row_width(gpu, spans, scale);
                let x = align_left(align_anchor(block.x, block.w, a), w, a).round();
                push_styled_row(gpu, spans, x, block.y, COLOR_WHITE, scale, texts, &mut |r, c| rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT }));
            }
            Self::end_text_area(gpu, texts);
        }

        // --- Schlaf-Hinweis: Vanilla zeigt im Bett einen eigenen Bildschirm mit "Bett
        // verlassen"-Knopf. Wir bleiben im Spiel und blenden nur ein, wie man wieder aufsteht.
        if self.sleeping_at.is_some() {
            let (sw, sh) = gpu.size();
            let text = "Schleichen: aufstehen".to_string();
            let w = gpu.measure_text(&text, UI_SCALE_NORMAL);
            texts.push(UiText { text, x: ((sw - w) / 2.0).round(), y: (sh * 0.62).round(), color: COLOR_WHITE, scale: UI_SCALE_NORMAL });
        }

        // --- Actionbar (Position aus dem HUD-Editor, s. hud_actionbar_rect) ---
        if let Some((spans, at)) = &self.action_bar {
            if at.elapsed() < ACTION_BAR_DURATION {
                self.begin_text_area(gpu, texts, ShadowArea::ActionBar);
                let rect = self.hud_actionbar_rect(gpu);
                // Ein UiText je Abschnitt, nebeneinandergesetzt -- eine Zeile kennt nur eine
                // Farbe (dieselbe Aufteilung wie im Chat). Ausgerichtet am Anker (s. HUD-Editor).
                let a = self.settings.hud_actionbar_align;
                let gesamt = span_row_width(gpu, spans, UI_SCALE_NORMAL);
                let x = align_left(align_anchor(rect.x, rect.w, a), gesamt, a).round();
                push_styled_row(gpu, spans, x, rect.y, COLOR_WHITE, UI_SCALE_NORMAL, texts, &mut |r, c| rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT }));
                Self::end_text_area(gpu, texts);
            }
        }
    }
}
