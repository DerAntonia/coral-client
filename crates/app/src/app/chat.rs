// Chat: Zeilen, Eingabe, Auswahl, Zwischenablage, Befehle und Chat-Sitzung.

use crate::*;

impl App {
    // Zeilenhoehe des Chats: Oberlaenge + UNTERLAENGE + ein Pixel Luft, je Font-Pixel mal
    // Faktor. Frueher stand hier die Zellhoehe (8) + 2 Bildschirmpixel -- mit DM Sans reicht
    // das "g" tiefer als die Minecraft-Zelle und beruehrte die naechste Zeile. Fuer
    // die Minecraft-Schrift ergibt das unveraendert Vanillas 9 Pixel.
    pub(crate) fn chat_line_height(&self, gpu: &GpuState) -> f32 {
        let abstand = 1.0 + self.settings.chat_line_spacing as f32 / 100.0;
        (self.chat_text_height(gpu) * abstand).round()
    }

    // So hoch wie der Text einer Chatzeile, ohne Zeilenabstand (Tooltips bleiben dicht).
    pub(crate) fn chat_text_height(&self, gpu: &GpuState) -> f32 {
        let f = &gpu.font_atlas;
        ((f.ascent_px() + f.descent_px() + 1.0) * self.chat_scale()).ceil()
    }

    // Bildschirm-Rechteck des GESCHLOSSENEN Chat-Nachrichtenblocks (ohne Eingabezeile).
    // hud_chat_x ist die normierte linke Kante, hud_chat_y die normierte UNTERKANTE -- der
    // Block waechst nach oben, deshalb ist die Unterkante der feste Bezugspunkt.
    //
    // Die Hoehe rechnet immer mit Settings::chat_lines vollen Zeilen, nicht mit der Zahl der
    // gerade vorhandenen Nachrichten: sonst wanderte der Griff im HUD-Editor bei jeder
    // eintreffenden Nachricht, und man koennte den leeren Chat gar nicht anfassen.
    pub(crate) fn hud_chat_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let line_h = self.chat_line_height(gpu);
        let h = self.settings.chat_lines as f32 * line_h;
        let w = (self.settings.chat_width as f32 * self.chat_scale()).min(sw).round();
        let x = (self.settings.hud_chat_x * sw).clamp(0.0, (sw - w).max(0.0)).round();
        let y = (self.settings.hud_chat_y * sh - h).clamp(0.0, (sh - h).max(0.0)).round();
        Rect { x, y, w, h }
    }

    // Der GEOEFFNETE Chat (T): rueckt direkt ueber die Eingabezeile, nimmt die ganze Breite
    // rechts von seiner linken Kante (die eingestellte Breite gilt nur geschlossen) und
    // Settings::chat_lines_open Zeilen -- so viele, wie ueber der Eingabezeile Platz haben.
    pub(crate) fn chat_open_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, _) = gpu.size();
        let line_h = self.chat_line_height(gpu);
        let input = self.chat_input_rect(gpu);
        let unten = input.y - 4.0;
        let x = (self.settings.hud_chat_x * sw).clamp(0.0, (sw - 100.0).max(0.0)).round();
        let zeilen = (self.settings.chat_lines_open as f32).min(((unten - 8.0) / line_h).floor()).max(1.0);
        let h = zeilen * line_h;
        Rect { x, y: (unten - h).round(), w: (sw - x - 8.0).max(50.0), h }
    }

    // Die Flaeche, die der offene Chat WIRKLICH bemalt. Zurueckgeblaettert nimmt er den
    // ganzen freien Platz unter sich mit, bis kurz ueber die Hotbar: unsere Eingabezeile
    // sitzt auf Fadenkreuz-Hoehe (s. chat_input_rect), mitten im Bild -- alles darunter
    // stuende beim Blaettern leer, waehrend oben Verlauf abgeschnitten wird.
    //
    // Die Bedingung liest das ROHE chat_scroll, nicht den geklemmten Stand aus
    // build_chat_overlay_body. Sonst haengt die Hoehe am Blaetterweg und der Blaetterweg an
    // der Hoehe: verlaengern kuerzt den Weg, die Klemmung zieht den Stand zurueck, das
    // Rechteck schrumpft wieder -- der Chat faengt an der obersten Nachricht an zu flackern.
    // Die Flaeche, die der offene Chat WIRKLICH bemalt -- Grundrechteck plus die Zeilen,
    // um die es beim Zurueckblaettern nach unten gewachsen ist (s. build_chat_overlay_body).
    pub(crate) fn chat_open_draw_rect(&self, gpu: &GpuState) -> Rect {
        let block = self.chat_open_rect(gpu);
        let line_h = self.chat_line_height(gpu);
        let max_wachstum = (self.chat_extend_max(gpu) / line_h).floor().max(0.0) as usize;
        let wachstum = self.chat_scroll.min(max_wachstum);
        Rect { h: block.h + wachstum as f32 * line_h, ..block }
    }

    // Wie weit der offene Chat hoechstens nach unten wachsen darf: von seiner Unterkante
    // (knapp ueber der Eingabezeile) bis knapp ueber die Hotbar. Unsere Eingabe sitzt auf
    // Fadenkreuz-Hoehe, mitten im Bild -- der Platz darunter stuende sonst leer.
    pub(crate) fn chat_extend_max(&self, gpu: &GpuState) -> f32 {
        let block = self.chat_open_rect(gpu);
        (self.hud_hotbar_rect(gpu).y - 4.0 - (block.y + block.h)).max(0.0)
    }

    // Alle Nachrichten gesetzt, aelteste zuerst -- fuer den geoeffneten Chat, der durch den
    // ganzen Verlauf blaettern kann. Zwischengespeichert: neu gesetzt wird nur, wenn eine
    // Nachricht dazukam oder sich Breite, Groesse oder Schrift geaendert haben.
    pub(crate) fn chat_wrapped_all(&self, gpu: &GpuState, width: f32, scale: f32) -> std::rc::Rc<Vec<ChatLine>> {
        let schrift = FontKind::ALL.iter().position(|&k| k == gpu.layout_atlas().kind()).unwrap_or(0) as u8;
        let key = (self.chat_generation, width.to_bits(), scale.to_bits(), schrift);
        if let Some((k, lines)) = self.chat_wrap_cache.borrow().as_ref() {
            if *k == key {
                return lines.clone();
            }
        }
        let messwerk = |t: &str| gpu.measure_text(t, scale);
        let lines: Vec<ChatLine> = self.chat_messages.iter().flat_map(|msg| wrap_chat_message(msg, width, messwerk)).collect();
        let lines = std::rc::Rc::new(lines);
        *self.chat_wrap_cache.borrow_mut() = Some((key, lines.clone()));
        lines
    }

    // Die juengsten `n` Zeilen fuer den geschlossenen Chat -- von hinten gesetzt, bis genug
    // beisammen sind, statt den ganzen Verlauf anzufassen.
    pub(crate) fn chat_wrapped_tail(&self, gpu: &GpuState, width: f32, scale: f32, n: usize) -> Vec<ChatLine> {
        let messwerk = |t: &str| gpu.measure_text(t, scale);
        let mut rueckwaerts: Vec<ChatLine> = Vec::new();
        for msg in self.chat_messages.iter().rev() {
            // Die Zeilen EINER Nachricht muessen ihre Reihenfolge behalten, sonst staende die
            // zweite ueber der ersten.
            rueckwaerts.extend(wrap_chat_message(msg, width, messwerk).into_iter().rev());
            if rueckwaerts.len() >= n {
                break;
            }
        }
        rueckwaerts.truncate(n);
        rueckwaerts.reverse();
        rueckwaerts
    }

    // Wie chat_wrapped_tail, aber fuer den geschlossenen Chat im Spiel: je Zeile die Deckkraft
    // ihrer Nachricht (s. chat_fade_alpha). Schon ausgeblendete Nachrichten fallen weg --
    // und mit ihnen alle aelteren, die sind ja noch frueher abgelaufen.
    pub(crate) fn chat_wrapped_tail_faded(&self, gpu: &GpuState, width: f32, scale: f32, n: usize) -> (Vec<ChatLine>, Vec<f32>) {
        let messwerk = |t: &str| gpu.measure_text(t, scale);
        let jetzt = Instant::now();
        let mut zeilen: Vec<ChatLine> = Vec::new();
        let mut deckkraft: Vec<f32> = Vec::new();
        for (msg, zeit) in self.chat_messages.iter().zip(&self.chat_times).rev() {
            let alpha = chat_fade_alpha(jetzt.duration_since(*zeit).as_secs_f32(), self.settings.chat_fade_secs);
            if alpha <= 0.0 {
                break;
            }
            for zeile in wrap_chat_message(msg, width, messwerk).into_iter().rev() {
                zeilen.push(zeile);
                deckkraft.push(alpha);
            }
            if zeilen.len() >= n {
                break;
            }
        }
        zeilen.truncate(n);
        deckkraft.truncate(n);
        zeilen.reverse();
        deckkraft.reverse();
        (zeilen, deckkraft)
    }

    // Chat im Spiel wie Bedrock: oben verankert. Jede Zeile hat einen festen Platz (ihre Nummer
    // seit Beginn, s. chat_lines_total); gezeigt werden die juengsten, die noch nicht
    // ausgeblendet sind, hoechstens chat_lines. Der Anfang gleitet: blendet oben eine Nachricht
    // aus, ruecken alle gleitend nach oben. Neue Nachrichten sind sofort da, auch wenn sie den
    // vollen Chat weiterschieben (s. tick_ui_anim).
    #[allow(clippy::too_many_arguments)]
    fn build_chat_bedrock(&self, gpu: &GpuState, block: Rect, scale: f32, line_h: f32, deckkraft: f32, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let max = self.settings.chat_lines as usize;
        // Doppelt so viele holen: was oben gerade hinausgleitet, ist auch noch zu sehen.
        let (lines, alphas) = self.chat_wrapped_tail_faded(gpu, block.w - 4.0, scale, 2 * max);
        let gesamt = self.chat_lines_total as f32;
        let lebend = lines.len().min(max) as f32;
        self.chat_bedrock_target.set((gesamt - lebend, gesamt));
        let (anfang, ende) = self.chat_bedrock_anim.unwrap_or((gesamt - lebend, gesamt));
        let unten = block.y + block.h;
        let n = lines.len();
        for (j, (line, a)) in lines.iter().zip(&alphas).enumerate() {
            let nummer = gesamt - (n - j) as f32;
            let herein = (ende - nummer).clamp(0.0, 1.0);
            let y = block.y + (nummer - anfang) * line_h + (1.0 - herein) * line_h;
            let deckung = a * herein * (1.0 - (y + line_h - unten) / line_h).clamp(0.0, 1.0) * (1.0 - (block.y - y) / line_h).clamp(0.0, 1.0);
            if deckung <= 0.0 {
                continue;
            }
            let zeile = Rect { y, h: line_h, ..block };
            if deckkraft > 0.0 {
                let (y0, y1) = (y.round(), (y + line_h).round());
                rects.push(UiRect { rect: Rect { y: y0, h: y1 - y0, ..block }, color: [0.0, 0.0, 0.0, deckkraft * deckung], ..UiRect::FLAT });
            }
            self.draw_chat_lines(gpu, std::slice::from_ref(line), zeile, scale, line_h, rects, texts, None, &[deckung], None);
        }
    }

    // Zeilen von unten nach oben in `rect` setzen. Links werden unterstrichen und -- wenn
    // `hits` da ist -- als Klickflaeche gemerkt (s. chat_click).
    pub(crate) fn draw_chat_lines(
        &self,
        gpu: &GpuState,
        lines: &[ChatLine],
        rect: Rect,
        scale: f32,
        line_h: f32,
        rects: &mut Vec<UiRect>,
        texts: &mut Vec<UiText>,
        mut hits: Option<&mut Vec<(Rect, ChatHit)>>,
        // Deckkraft je Zeile (s. chat_wrapped_tail_faded); leer = alles voll.
        deckkraft: &[f32],
        // Unterkante, ab der nichts mehr gezeichnet wird. Die Zeilen behalten dabei ihre
        // Lage -- der Verlauf wird ABGESCHNITTEN, nicht neu umgebrochen (s. Aufrufstelle im
        // offenen Chat: sobald man tippt, holt sich die Eingabezeile ihren Platz zurueck).
        max_bottom: Option<f32>,
    ) {
        let top = rect.y + rect.h - lines.len() as f32 * line_h;
        let underline_y = (gpu.font_atlas.ascent_px() + 1.0) * scale;
        let underline_h = (scale / 2.0).round().max(1.0);
        // Mit Zeilenabstand steht der Text mittig in seiner Zeile.
        let text_dy = ((line_h - self.chat_text_height(gpu)) / 2.0).max(0.0).round();
        for (i, line) in lines.iter().enumerate() {
            let y = top + i as f32 * line_h;
            let ty = y + text_dy;
            if max_bottom.is_some_and(|unten| y + line_h > unten) {
                break;
            }
            let alpha = deckkraft.get(i).copied().unwrap_or(1.0);
            let mut x = rect.x + 2.0 + line.indent;
            for span in &line.spans {
                let mut farbe = span.color.map(rgb_to_rgba).unwrap_or(COLOR_WHITE);
                farbe[3] *= alpha;
                let w = gpu.measure_text(&span.text, scale);
                push_span_shadow(texts, span.shadow);
                texts.push(UiText { text: span.text.clone(), x: x.round(), y: ty, color: farbe, scale });
                if span.link.is_some() {
                    rects.push(UiRect { rect: Rect { x: x.round(), y: (ty + underline_y).round(), w: w.round(), h: underline_h }, color: farbe, ..UiRect::FLAT });
                }
                if span.link.is_some() || span.action.is_some() {
                    if let Some(hits) = hits.as_deref_mut() {
                        hits.push((Rect { x, y, w, h: line_h }, ChatHit { link: span.link.clone(), action: span.action.clone() }));
                    }
                }
                x += w;
            }
        }
    }

    // Vanillas Tooltip-Kasten am Mauszeiger (DrawContext.drawHoverEvent / TooltipComponent):
    // 12 Pixel rechts oberhalb des Zeigers, am rechten Rand auf die andere Seite geklappt,
    // Text auf die halbe Bildschirmbreite umbrochen (mindestens 200). Farben wie Vanillas
    // TooltipBackgroundRenderer: fast deckendes Dunkelviolett, lila Rahmen.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_chat_tooltip(
        &self,
        gpu: &GpuState,
        tooltip: &[protocol::nbt::TextSpan],
        scale: f32,
        line_h: f32,
        rects: &mut Vec<UiRect>,
        texts: &mut Vec<UiText>,
        texte_ab: usize,
        flaechen_ab: usize,
    ) {
        let (sw, sh) = gpu.size();
        let messwerk = |t: &str| gpu.measure_text(t, scale);
        let zeilen = wrap_label_lines(&split_label_lines(tooltip), (sw / 2.0).max(200.0 * scale), messwerk);
        if zeilen.is_empty() {
            return;
        }
        let pad = (4.0 * scale).round();
        let rand = scale.round().max(1.0);
        let breite = zeilen.iter().map(|z| z.iter().map(|s| messwerk(&s.text)).sum::<f32>()).fold(0.0f32, f32::max) + 2.0 * pad;
        let hoehe = zeilen.len() as f32 * line_h + 2.0 * pad;
        let (cx, cy) = self.cursor_pos;
        let versatz = 12.0 * scale;
        let mut x = cx + versatz;
        if x + breite > sw - 4.0 {
            x = cx - versatz - breite;
        }
        let x = x.clamp(4.0, (sw - breite - 4.0).max(4.0)).round();
        let y = (cy - versatz).clamp(4.0, (sh - hoehe - 4.0).max(4.0)).round();
        let kasten = Rect { x, y, w: breite.round(), h: hoehe.round() };

        cut_ui_under(gpu, texts, texte_ab, rects, flaechen_ab, kasten);
        let mut rahmen = rgb_to_rgba(0x5000FF);
        rahmen[3] = 0.6;
        let mut grund = rgb_to_rgba(0x100010);
        grund[3] = 0.94;
        rects.push(UiRect { rect: Rect { x: x - rand, y: y - rand, w: kasten.w + 2.0 * rand, h: kasten.h + 2.0 * rand }, color: rahmen, ..UiRect::FLAT });
        rects.push(UiRect { rect: kasten, color: grund, ..UiRect::FLAT });
        for (i, zeile) in zeilen.iter().enumerate() {
            let mut tx = x + pad;
            let ty = y + pad + i as f32 * line_h;
            for span in zeile {
                push_span_shadow(texts, span.shadow);
                texts.push(UiText { text: span.text.clone(), x: tx.round(), y: ty, color: span.color.map(rgb_to_rgba).unwrap_or(COLOR_WHITE), scale });
                tx += messwerk(&span.text);
            }
        }
    }

    // Schreibcursor-Spalte der Chat-Eingabe: etwas rechts der Bildschirmmitte (s.
    // CHAT_CARET_OFFSET_X). Links davon steht der Text, rechts davon die Verlaufsliste.
    pub(crate) fn chat_caret_x(&self, gpu: &GpuState) -> f32 {
        (gpu.size().0 / 2.0 + CHAT_CARET_OFFSET_X).round()
    }

    // Wo Zeile `i` der Eingabe endet (s. ChatGap): alle am Anker, die letzte um den Abstand
    // weiter links, den Loeschen dort hinterlassen hat.
    pub(crate) fn chat_line_end(&self, gpu: &GpuState, text: &str, i: usize) -> f32 {
        let zeilen = self.chat_input_lines(gpu, text);
        let anker = self.chat_caret_x(gpu);
        if i + 1 != zeilen.len() {
            return anker;
        }
        let w = zeilen.last().map_or(0.0, |z| gpu.measure_raw(z, UI_SCALE_NORMAL));
        let platz = (anker - 6.0).max(20.0);
        let mut s = self.chat_gap.borrow_mut();
        let gap = match s.as_ref() {
            // Alles geloescht: der Cursor kehrt an den Anker zurueck.
            _ if text.is_empty() => 0.0,
            Some(g) if g.text == text => g.gap,
            // Gleich viele Zeilen: was die letzte schmaler wurde, waechst als Abstand, was sie
            // breiter wurde, zehrt ihn auf. Bricht der Text anders um, steht sie wieder am Anker.
            Some(g) if g.lines == zeilen.len() => (g.gap + g.last_w - w).clamp(0.0, (platz - w).max(0.0)),
            _ => 0.0,
        };
        *s = Some(ChatGap { text: text.to_string(), lines: zeilen.len(), last_w: w, gap });
        anker - gap
    }

    pub(crate) fn chat_input_line_h(gpu: &GpuState) -> f32 {
        gpu.text_height(UI_SCALE_NORMAL) + 2.0
    }

    // Die Zeilen des Eingabefeldes: der Text, umbrochen auf den Platz links vom Cursor.
    pub(crate) fn chat_input_lines<'a>(&self, gpu: &GpuState, text: &'a str) -> Vec<&'a str> {
        let platz = (self.chat_caret_x(gpu) - 6.0).max(20.0);
        chat_input::wrap_input_lines(text, platz, |t| gpu.measure_raw(t, UI_SCALE_NORMAL))
    }

    // Wo die Chat-Eingabe sitzt: die ERSTE Zeile senkrecht zentriert auf der Fadenkreuz-
    // Position (Bildmitte) -- das Fadenkreuz ist beim Tippen unsichtbar, die
    // Eingabe ersetzt es an derselben Stelle. Waagerecht vom linken Rand bis knapp hinter den
    // Schreibcursor. Ist eine Zeile voll, waechst das Feld nach UNTEN um eine Zeile.
    pub(crate) fn chat_input_rect(&self, gpu: &GpuState) -> Rect {
        let (_, screen_h) = gpu.size();
        let line_h = Self::chat_input_line_h(gpu);
        let zeilen = self.chat_input.as_ref().map_or(1, |f| self.chat_input_lines(gpu, f.text()).len()) as f32;
        let y = (screen_h / 2.0 - (line_h + 6.0) / 2.0).round();
        Rect { x: 0.0, y, w: self.chat_caret_x(gpu) + CARET_WIDTH_PX + 6.0, h: zeilen * line_h + 6.0 }
    }

    // Die Liste der gesendeten Nachrichten: rechts neben der Eingabe, von ihrer Oberkante bis
    // zum unteren Bildschirmrand.
    pub(crate) fn chat_history_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let input = self.chat_input_rect(gpu);
        let x = input.x + input.w + 16.0;
        Rect { x, y: input.y, w: (sw - 8.0 - x).max(0.0), h: (sh - 8.0 - input.y).max(0.0) }
    }

    pub(crate) fn build_chat_overlay(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        // Der Tooltip kommt ganz zuletzt, damit er auch ueber Eingabe, Scrollbalken und
        // Vorschlaegen liegt -- ausgespart wird aber nur, was NACH dem Chat-Hintergrund kam.
        if let Some((tooltip, texte_ab, flaechen_ab)) = self.build_chat_overlay_body(gpu, rects, texts) {
            let scale = self.chat_scale();
            self.build_chat_tooltip(gpu, &tooltip, scale, self.chat_text_height(gpu), rects, texts, texte_ab, flaechen_ab);
        }
    }

    // Liefert den Tooltip unter dem Zeiger samt den Stellen in `texts`/`rects`, ab denen er
    // Chat-Inhalt verdeckt.
    pub(crate) fn build_chat_overlay_body(
        &self,
        gpu: &GpuState,
        rects: &mut Vec<UiRect>,
        texts: &mut Vec<UiText>,
    ) -> Option<(Vec<protocol::nbt::TextSpan>, usize, usize)> {
        let mut tooltip_plan = None;
        let scale = self.chat_scale();
        let line_h = self.chat_line_height(gpu);
        let dunkel = |alpha: f32| [0.0, 0.0, 0.0, alpha];
        let deckkraft = self.settings.chat_background as f32 / 100.0;

        if self.chat_input.is_none() {
            // GESCHLOSSEN: die juengsten Zeilen am eingestellten Ort, Hintergrund nur hinter
            // Zeilen, die es auch gibt.
            let block = self.hud_chat_rect(gpu);
            if self.settings.chat_bedrock {
                self.build_chat_bedrock(gpu, block, scale, line_h, deckkraft, rects, texts);
                return None;
            }
            // Neue Nachrichten gleiten von unten herein (s. chat_rise): alles steht um `rise`
            // Zeilen zu tief, unten blendet die neue Zeile ein, oben die verdraengte aus.
            let rise = self.chat_rise;
            let (lines, mut alphas) = self.chat_wrapped_tail_faded(gpu, block.w - 4.0, scale, self.settings.chat_lines as usize + rise.ceil() as usize);
            let unten = block.y + block.h;
            let flaeche = Rect { y: block.y + rise * line_h, ..block };
            for (i, a) in alphas.iter_mut().enumerate() {
                let y = flaeche.y + flaeche.h - (lines.len() - i) as f32 * line_h;
                let unten_drin = 1.0 - (y + line_h - unten) / line_h;
                let oben_drin = 1.0 - (block.y - y) / line_h;
                *a *= unten_drin.clamp(0.0, 1.0) * oben_drin.clamp(0.0, 1.0);
            }
            if !lines.is_empty() {
                if deckkraft > 0.0 {
                    // Je Zeile ein Streifen, damit der Hintergrund mit seiner Nachricht ausblendet.
                    let top = flaeche.y + flaeche.h - lines.len() as f32 * line_h;
                    for (i, alpha) in alphas.iter().enumerate() {
                        let y = (top + i as f32 * line_h).round();
                        let y2 = (top + (i + 1) as f32 * line_h).round();
                        rects.push(UiRect { rect: Rect { x: block.x, y, w: block.w, h: y2 - y }, color: dunkel(deckkraft * alpha), ..UiRect::FLAT });
                    }
                }
                self.draw_chat_lines(gpu, &lines, flaeche, scale, line_h, rects, texts, None, &alphas, None);
            }
        } else {
            // OFFEN: ueber der Eingabezeile, volle Breite, mit Mausrad durch den ganzen Verlauf
            // (s. scroll_chat), Links anklickbar (s. chat_click).
            // Alles ueber der Eingabezeile UND sie selbst abdunkeln (Settings::chat_open_dim). Ganz vorn in
            // die Flaechenliste: es liegt ueber der Welt, aber unter allen HUD-Flaechen.
            let dim = self.settings.chat_open_dim as f32 / 100.0;
            if dim > 0.0 {
                // Bis einschliesslich der Eingabezeile (auch wenn sie mehrzeilig gewachsen ist).
                let eingabe = self.chat_input_rect(gpu);
                rects.insert(0, UiRect { rect: Rect { x: 0.0, y: 0.0, w: gpu.size().0, h: eingabe.y + eingabe.h }, color: dunkel(dim), ..UiRect::FLAT });
            }
            let block = self.chat_open_rect(gpu);
            let alle = self.chat_wrapped_all(gpu, block.w - 12.0, scale);
            let sichtbar_basis = (block.h / line_h).round().max(1.0) as usize;
            let max_scroll = alle.len().saturating_sub(sichtbar_basis);
            let scroll = self.chat_scroll.min(max_scroll);
            // Gezeichnet wird gleitend: Ausschnitt fuer die ganze Zeile darunter, eine Zeile
            // mehr oben, alles um den Bruchteil tiefer (s. unten).
            let gleitend = self.chat_scroll_glide.stand.clamp(0.0, max_scroll as f32);
            let ganz = gleitend.floor() as usize;

            // Solange unter dem Chat noch Platz ist, kostet eine Radstufe KEINE Zeile: der
            // Block waechst stattdessen um eine Zeile nach unten. Die unterste Nachricht
            // rueckt also eine Zeile tiefer und bleibt stehen, waehrend oben eine aeltere
            // dazukommt -- nichts faellt hinten weg, bis der Platz aufgebraucht ist. Erst
            // danach blaettert es wie gewohnt.
            //
            // Wichtig fuer die Lage des Textes: die oberste Zeile bleibt dabei genau da, wo
            // sie war, und alles darunter wandert um GENAU eine Zeile. Deshalb braucht das
            // keine Animation -- es bewegt sich von sich aus mit der Radgeschwindigkeit.
            let max_wachstum = (self.chat_extend_max(gpu) / line_h).floor().max(0.0) as usize;
            let (wachstum, anfang, ende) = chat_scroll_layout(alle.len(), sichtbar_basis, max_wachstum, ganz);
            let sichtbar = sichtbar_basis + wachstum;
            let wachstum_f = gleitend.min(max_wachstum as f32);
            let zeichenflaeche = Rect { h: block.h + wachstum_f * line_h, ..block };

            // Fuer die mitwandernde Textauswahl zaehlt, wo der Text STEHT, nicht welcher
            // Ausschnitt gezeigt wird. Beides zusammen -- Blaettern und Wachsen -- ergibt
            // denselben Bezugspunkt wie frueher: alle.len() - scroll.
            self.chat_view.set(Some((self.chat_dropped_lines + alle.len() - scroll, line_h)));

            // Sobald etwas getippt ist, holt sich die Eingabe ihren Platz zurueck und der
            // Verlauf wird genau an ihrer Oberkante abgeschnitten: NICHT neu umgebrochen --
            // die Zeilen darueber bleiben stehen, nur was darunter lag faellt weg.
            let getippt = self.chat_input.as_ref().is_some_and(|f| !f.text().is_empty());
            let abschneiden = if getippt && wachstum_f > 0.0 { Some(block.y + block.h) } else { None };
            // Hintergrund und Scrollbalken folgen dem, was man WIRKLICH sieht: beim
            // Abschneiden wieder nur bis zur Eingabezeile, sonst bis ganz hinunter.
            let sichtbereich = if abschneiden.is_some() { block } else { zeichenflaeche };
            if deckkraft > 0.0 {
                rects.push(UiRect { rect: sichtbereich, color: dunkel(deckkraft), ..UiRect::FLAT });
            }
            let (texte_ab, flaechen_ab) = (texts.len(), rects.len());
            let mut hits = Vec::new();
            // Was das Wachsen nicht schon nach unten geschoben hat, rueckt der Text selbst: in
            // Summe immer genau der Bruchteil einer Zeile. Die Zeile, die oben hereingleitet,
            // erscheint erst, wenn sie ganz drin ist; unten verschwindet die herausgleitende.
            // Ganz unten gleiten neue Nachrichten von unten herein wie im Spiel (s. chat_rise).
            // Zeilen am Rand blenden anteilig aus, statt zu springen.
            let rise = if self.chat_scroll == 0 { self.chat_rise_open } else { 0.0 };
            let schub = ((gleitend - ganz as f32) - (wachstum_f - wachstum as f32) + rise) * line_h;
            let flaeche = Rect { y: zeichenflaeche.y + schub, ..zeichenflaeche };
            let (oben, unten) = (zeichenflaeche.y, zeichenflaeche.y + zeichenflaeche.h);
            let mut von = anfang.saturating_sub(1 + rise.ceil() as usize);
            let deckung = |i: usize, von: usize| {
                let y = flaeche.y + flaeche.h - (ende - von - i) as f32 * line_h;
                (1.0 - (y + line_h - unten) / line_h).clamp(0.0, 1.0) * (1.0 - (oben - y) / line_h).clamp(0.0, 1.0)
            };
            // Ganz verdeckte Zeilen oben fallen weg (keine unsichtbaren Links); unten haelt
            // bis_unten sie ab.
            while von < ende && deckung(0, von) <= 0.0 {
                von += 1;
            }
            let alphas: Vec<f32> = (0..ende - von).map(|i| deckung(i, von)).collect();
            let bis_unten = unten + line_h - 0.01;
            let bis_unten = Some(abschneiden.map_or(bis_unten, |a| a.min(bis_unten)));
            self.draw_chat_lines(gpu, &alle[von..ende], flaeche, scale, line_h, rects, texts, Some(&mut hits), &alphas, bis_unten);
            // Tooltip des Abschnitts unter dem Zeiger (HoverEvent, s. nbt::SpanAction::hover).
            let (cx, cy) = self.cursor_pos;
            let tooltip = hits.iter().find(|(r, _)| r.contains(cx, cy)).and_then(|(_, h)| h.action.as_ref()?.hover.clone());
            tooltip_plan = tooltip.map(|t| (t, texte_ab, flaechen_ab));
            *self.chat_link_hits.borrow_mut() = hits;
            // Scrollbalken rechts, nur wenn es mehr gibt, als hineinpasst.
            if max_scroll > 0 {
                let spur = Rect { x: sichtbereich.x + sichtbereich.w - 6.0, y: sichtbereich.y, w: 4.0, h: sichtbereich.h };
                rects.push(UiRect { rect: spur, color: [1.0, 1.0, 1.0, 0.12], ..UiRect::FLAT });
                let anteil = sichtbar as f32 / alle.len() as f32;
                let daumen_h = (spur.h * anteil).max(12.0);
                let pos = 1.0 - gleitend / max_scroll as f32;
                rects.push(UiRect {
                    rect: Rect { x: spur.x, y: spur.y + (spur.h - daumen_h) * pos, w: spur.w, h: daumen_h },
                    color: [1.0, 1.0, 1.0, 0.55],
                    ..UiRect::FLAT
                });
            }
        }

        // Die Eingabe sitzt an der Fadenkreuz-Position (s. chat_input_rect), NICHT am
        // Nachrichtenblock. Der Schreibcursor steht fest knapp rechts der Mitte; jede Zeile
        // steht rechtsbuendig an ihm. Ist eine Zeile voll (linker Rand bis Cursor), geht es
        // in einer neuen Zeile darunter weiter -- der Cursor wandert mit nach unten.
        let Some(feld) = self.chat_input.as_ref() else { return tooltip_plan };
        selection_boundary(texts);
        let (screen_w, _) = gpu.size();
        let rect = self.chat_input_rect(gpu);
        if deckkraft > 0.0 {
            rects.push(UiRect { rect, color: dunkel(deckkraft), ..UiRect::FLAT });
        }
        let line_h = Self::chat_input_line_h(gpu);
        let caret_x = self.chat_caret_x(gpu);
        let zeilen = self.chat_input_lines(gpu, feld.text());
        // Die Zeile mit dem Cursor zerfaellt an ihm, solange sie gleitet: links gleitet (s.
        // ChatSlide), rechts steht. Sonst bleibt sie EIN Text -- zwei getrennt gemessene Teile
        // liegen um ein Pixel anders als die ganze Zeile, und beim Verschieben des Cursors
        // wanderte die Trennstelle und mit ihr dieses Pixel: die Buchstaben wackelten.
        let (cursor_zeile, cursor_x) = self.chat_input_spot(gpu, feld.text(), feld.cursor(), false);
        let zeile_start = |z: &str| z.as_ptr() as usize - feld.text().as_ptr() as usize;
        let ende = |i: usize| self.chat_line_end(gpu, feld.text(), i);
        let versatz = zeilen.get(cursor_zeile).map_or(0.0, |z| self.chat_slide_offset(feld.text(), cursor_zeile, ende(cursor_zeile) - gpu.measure_raw(z, UI_SCALE_NORMAL)));
        // Blaettern im Verlauf: k von 0 bis 1, der neue Text kommt aus `richtung` Zeilen Abstand.
        let recall = self.chat_recall.as_ref().and_then(|r| {
            let t = r.start.elapsed().as_secs_f32() / CHAT_RECALL.as_secs_f32();
            (t < 1.0).then(|| (r, ease_out(t), if r.nach_oben { 1.0 } else { -1.0 }))
        });
        let (neu_dy, neu_alpha) = recall.map_or((0.0, 1.0), |(_, k, richtung)| (richtung * line_h * (1.0 - k), k));
        if let Some((r, k, richtung)) = recall {
            for (i, zeile) in self.chat_input_lines(gpu, &r.alt).iter().enumerate() {
                let y = rect.y + 3.0 + i as f32 * line_h + 1.0 - richtung * line_h * k;
                let x0 = caret_x - gpu.measure_raw(zeile, UI_SCALE_NORMAL);
                texts.push(UiText { text: zeile.to_string(), x: x0.round(), y: y.round(), color: [1.0, 1.0, 1.0, 1.0 - k], scale: UI_SCALE_NORMAL });
            }
        }
        for (i, zeile) in zeilen.iter().enumerate() {
            let y = (rect.y + 3.0 + i as f32 * line_h + 1.0 + neu_dy).round();
            let x0 = ende(i) - gpu.measure_raw(zeile, UI_SCALE_NORMAL);
            let teile: Vec<(&str, f32)> = if i == cursor_zeile && versatz.abs() >= 0.5 {
                let k = feld.cursor().saturating_sub(zeile_start(zeile)).min(zeile.len());
                let (links, rechts) = zeile.split_at(k);
                // Der rechte Teil dort, wo er in der ganzen Zeile stuende (s. oben).
                vec![(links, x0 + versatz), (rechts, x0 + gpu.measure_raw(links, UI_SCALE_NORMAL))]
            } else {
                vec![(zeile, x0)]
            };
            for (teil, x) in teile.into_iter().filter(|(t, _)| !t.is_empty()) {
                push_typed_marker(texts);
                let [r, g, b, a] = COLOR_WHITE;
                texts.push(UiText { text: teil.to_string(), x: x.round(), y, color: [r, g, b, a * neu_alpha], scale: UI_SCALE_NORMAL });
            }
        }
        // Cursor -- mit derselben Rate, Breite und Farbe wie in den Zahlenfeldern (s.
        // push_caret): zwei verschieden blinkende Cursor im selben Programm saehen nach einem
        // Fehler aus. Steht an der Cursorstelle im Text.
        let text_y = rect.y + 3.0 + cursor_zeile as f32 * line_h + 1.0;
        self.push_caret(rects, gpu.caret_rect(cursor_x.round(), text_y, UI_SCALE_NORMAL));

        selection_boundary(texts);
        self.build_chat_history_list(gpu, feld.history(), deckkraft, rects, texts);
        selection_boundary(texts);

        // Vorschlagsliste UEBER dem Feld, linksbuendig am Cursor -- dort, wo das Wort steht,
        // das ergaenzt wird. Der gewaehlte Eintrag ist hervorgehoben (Pfeile blaettern, Tab
        // setzt ein, s. chat_input::ChatInput::step/apply_selected).
        if feld.has_suggestions() {
            let liste: Vec<&String> = feld.suggestions().iter().take(chat_input::MAX_VISIBLE_SUGGESTIONS).collect();
            let zeile_h = gpu.text_height(UI_SCALE_NORMAL) + 2.0;
            let breite = liste.iter().map(|s| gpu.measure_text(s, UI_SCALE_NORMAL)).fold(40.0f32, f32::max) + 8.0;
            let unten = rect.y - 2.0;
            let oben = unten - liste.len() as f32 * zeile_h;
            let x = caret_x.min(screen_w - breite);
            rects.push(UiRect { rect: Rect { x, y: oben, w: breite, h: liste.len() as f32 * zeile_h }, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            for (i, eintrag) in liste.iter().enumerate() {
                let y = oben + i as f32 * zeile_h;
                let gewaehlt = i == feld.selected();
                if gewaehlt {
                    rects.push(UiRect { rect: Rect { x, y, w: breite, h: zeile_h }, color: theme::get(theme::Slot::AccentSurface), ..UiRect::FLAT });
                }
                texts.push(UiText {
                    text: (*eintrag).clone(),
                    x: x + 4.0,
                    y: y + 1.0,
                    color: if gewaehlt { COLOR_WHITE } else { theme::text_disabled() },
                    scale: UI_SCALE_NORMAL,
                });
            }
        }
        tooltip_plan
    }

    // Die gesendeten Nachrichten rechts neben der Eingabe (s. chat_history_rect), NEUESTE
    // OBEN -- aeltere erreicht man durch Herunterscrollen (s. scroll_chat_history). Ein Klick
    // setzt den Eintrag in die Eingabe (s. chat_click).
    pub(crate) fn build_chat_history_list(&self, gpu: &GpuState, verlauf: &[String], deckkraft: f32, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let mut hits = Vec::new();
        let flaeche = self.chat_history_rect(gpu);
        let zeile_h = gpu.text_height(UI_SCALE_SMALL) + 4.0;
        let platz = (flaeche.h / zeile_h).floor() as usize;
        if verlauf.is_empty() || platz == 0 || flaeche.w < 40.0 {
            *self.chat_history_hits.borrow_mut() = hits;
            return;
        }
        // Neueste zuerst: `scroll` Eintraege vom neuesten Ende uebersprungen, dann so viele
        // aeltere, wie Platz haben.
        let max_scroll = verlauf.len().saturating_sub(platz);
        let gleitend = self.chat_history_glide.stand.clamp(0.0, max_scroll as f32);
        let eintraege: Vec<(usize, &String)> = verlauf.iter().rev().enumerate().skip(gleitend.floor() as usize).take(platz + 1).collect();
        let h = verlauf.len().min(platz) as f32 * zeile_h;
        if deckkraft > 0.0 {
            rects.push(UiRect { rect: Rect { h, ..flaeche }, color: [0.0, 0.0, 0.0, deckkraft], ..UiRect::FLAT });
        }
        let textbreite = flaeche.w - 12.0;
        let (mx, my) = self.cursor_pos;
        for (k, eintrag) in eintraege {
            // Gleitend; nur ganz sichtbare Eintraege.
            let zeile = Rect { x: flaeche.x, y: flaeche.y + (k as f32 - gleitend) * zeile_h, w: flaeche.w, h: zeile_h };
            if zeile.y < flaeche.y - 0.5 || zeile.y + zeile_h > flaeche.y + h + 0.5 {
                continue;
            }
            if zeile.contains(mx, my) {
                rects.push(UiRect { rect: zeile, color: [1.0, 1.0, 1.0, 0.12], ..UiRect::FLAT });
            }
            // Zu lange Eintraege hinten kuerzen.
            let mut sichtbar = eintrag.clone();
            if gpu.measure_text(&sichtbar, UI_SCALE_SMALL) > textbreite {
                while !sichtbar.is_empty() && gpu.measure_text(&format!("{sichtbar}..."), UI_SCALE_SMALL) > textbreite {
                    sichtbar.pop();
                }
                sichtbar.push_str("...");
            }
            texts.push(UiText { text: sichtbar, x: zeile.x + 4.0, y: zeile.y + 2.0, color: COLOR_WHITE, scale: UI_SCALE_SMALL });
            hits.push((zeile, eintrag.clone()));
        }
        // Scrollbalken, wenn nicht alles hineinpasst -- oben = neueste.
        if max_scroll > 0 {
            let spur = Rect { x: flaeche.x + flaeche.w - 4.0, y: flaeche.y, w: 3.0, h };
            rects.push(UiRect { rect: spur, color: [1.0, 1.0, 1.0, 0.12], ..UiRect::FLAT });
            let daumen_h = (h * platz as f32 / verlauf.len() as f32).max(10.0);
            let pos = gleitend / max_scroll as f32;
            rects.push(UiRect { rect: Rect { y: spur.y + (h - daumen_h) * pos, h: daumen_h, ..spur }, color: [1.0, 1.0, 1.0, 0.55], ..UiRect::FLAT });
        }
        *self.chat_history_hits.borrow_mut() = hits;
    }

    // Einzeiliges Eingabefeld (Server-Fenster, Quick Connect) in `feld`: Markierung, Text und,
    // wenn es den Fokus hat, der Cursor. Den Rahmen zeichnet der Aufrufer. Beim Markieren bleibt
    // der Cursor stehen (s. TextField::caret), die Markierung waechst aus ihm heraus.
    pub(crate) fn push_text_field(&self, gpu: &GpuState, tf: &text_field::TextField, feld: Rect, fokus: bool, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let wert = tf.text();
        let h = gpu.text_height(UI_SCALE_NORMAL);
        let ty = (feld.y + (feld.h - h) / 2.0).round();
        let bis = |i: usize| feld.x + 12.0 + gpu.measure_raw(&wert[..i], UI_SCALE_NORMAL);
        let caret_x = bis(tf.caret());
        if let Some(r) = tf.selection().filter(|_| fokus) {
            let (x0, x1) = (bis(r.start), bis(r.end));
            self.push_field_selection(rects, Rect { x: x0, y: ty, w: x1 - x0, h }, caret_x);
        }
        push_typed_marker(texts);
        texts.push(UiText { text: wert.to_string(), x: feld.x + 12.0, y: ty, color: theme::text(), scale: UI_SCALE_NORMAL });
        if fokus {
            self.push_caret(rects, gpu.caret_rect(caret_x, ty, UI_SCALE_NORMAL));
        }
    }

    // Markierte Flaeche `f` in der Theme-Farbe "Markierung", an der linken und rechten Kante mit
    // dem Schein (s. SELECTION_GLOW_W) in Staerke `glow` (links, rechts; 0..1): zur Kante hin
    // linear heller. Nur der Schein ist in Streifen zerlegt, der Rest ist eine Flaeche.
    pub(crate) fn push_selection_span(&self, rects: &mut Vec<UiRect>, f: Rect, glow: (f32, f32)) {
        let nach = theme::with_alpha(theme::get(theme::Slot::Selection), theme::SELECTION_ALPHA);
        let von = theme::with_alpha(theme::text(), SELECTION_FLASH_ALPHA);
        let (x0, x1) = (f.x, f.x + f.w);
        if x1 <= x0 {
            return;
        }
        let (gl, gr) = glow;
        let links_bis = if gl > 0.001 { (x0 + SELECTION_GLOW_W).min(x1) } else { x0 };
        let rechts_ab = if gr > 0.001 { (x1 - SELECTION_GLOW_W).max(links_bis) } else { x1 };
        if rechts_ab > links_bis {
            rects.push(UiRect { rect: Rect { x: links_bis, w: rechts_ab - links_bis, ..f }, color: nach, ..UiRect::FLAT });
        }
        let schein = |rects: &mut Vec<UiRect>, von_x: f32, bis_x: f32| {
            let mut a = von_x;
            while a < bis_x {
                let b = ((a / SELECTION_SLICE).floor() * SELECTION_SLICE + SELECTION_SLICE).min(bis_x);
                let mitte = (a + b) / 2.0;
                let l = (1.0 - (mitte - x0) / SELECTION_GLOW_W).max(0.0) * gl;
                let r = (1.0 - (x1 - mitte) / SELECTION_GLOW_W).max(0.0) * gr;
                rects.push(UiRect { rect: Rect { x: a, w: b - a, ..f }, color: mix_color(nach, von, l.max(r)), ..UiRect::FLAT });
                a = b;
            }
        };
        schein(rects, x0, links_bis);
        schein(rects, rechts_ab, x1);
    }

    // Markierung eines Eingabefelds an `ziel`: gleitet in CARET_GLIDE dorthin -- die erste aus
    // dem Zeiger heraus, wenn er ueber ihr steht (Ziehen, Doppelklick mitten ins Wort: dann
    // waechst sie in beide Richtungen), sonst aus dem Cursor bei `caret_x` (Tastatur); jede
    // weitere von dort, wo die vorige gerade stand.
    pub(crate) fn push_field_selection(&self, rects: &mut Vec<UiRect>, ziel: Rect, caret_x: f32) {
        let (px, py) = self.cursor_pos;
        let unter_zeiger = py >= ziel.y && py < ziel.y + ziel.h && px >= ziel.x && px <= ziel.x + ziel.w;
        let anfang = if unter_zeiger { Rect { x: px, w: 0.0, ..ziel } } else { Rect { x: caret_x, w: CARET_WIDTH_PX, ..ziel } };
        let (f, glow) = self.glide_selection(ziel, anfang);
        self.push_selection_span(rects, f, glow);
    }

    // UI-Klaenge der Markierung, einmal je Bild aus der Zahl markierter Zeichen: aendert sie
    // sich, ploppt es (Popup) -- bei einem Doppelklick (Wort) stattdessen Step, bei einem
    // Dreifachklick (Zeile) Click. Hebt ein Mausklick oder Strg+D sie auf, kommt Erase; durch
    // Tippen oder Seitenwechsel bleibt es still.
    // Das Ploppen beim Zeichenweise-Markieren steigt wie in osu! (OsuTextBox.PlayFeedbackSample)
    // mit dem markierten Anteil am Text: Wiedergabe um bis zu 15 % schneller, ganz markiert
    // also gut 2,4 Halbtoene hoeher.
    pub(crate) fn selection_sounds(&mut self) {
        let (jetzt, gesamt) = self.selection_extent();
        let vorher = std::mem::replace(&mut self.selection_sound_chars, jetzt);
        let hinweis = self.selection_sound_hint.take();
        let klick = std::mem::take(&mut self.selection_sound_deselect);
        if jetzt > 0 && (jetzt != vorher || hinweis.is_some()) {
            match hinweis {
                Some(klang) => self.play_ui_sound(klang),
                None => {
                    let anteil = jetzt as f32 / gesamt.max(1) as f32;
                    self.play_ui_sound_pitched(ui_sound::UiSound::Popup, 12.0 * (1.0 + 0.15 * anteil.min(1.0)).log2());
                }
            }
        } else if jetzt == 0 && vorher > 0 && klick {
            self.play_ui_sound(ui_sound::UiSound::Erase);
        }
    }

    // Wie viele Zeichen gerade markiert sind (im Eingabefeld, in der Chat-Eingabe, im
    // Chatverlauf oder Log) und wie lang der Text ist, in dem markiert wird: das Feld, die
    // Chat-Eingabe, sonst die Zeilen, die die Markierung beruehrt.
    pub(crate) fn selection_extent(&mut self) -> (usize, usize) {
        let zeichen = |t: &str, r: std::ops::Range<usize>| (t[r].chars().count(), t.chars().count());
        if let Some(f) = self.active_text_field() {
            if let Some(r) = f.selection() {
                return zeichen(f.text(), r);
            }
        }
        if let Some(c) = self.chat_input.as_ref() {
            if let Some(r) = c.keyboard_selection() {
                return zeichen(c.text(), r);
            }
        }
        if self.text_selection.is_none() {
            return (0, 0);
        }
        if let (Some(r), Some(c)) = (self.chat_input_selection(), self.chat_input.as_ref()) {
            return zeichen(c.text(), r);
        }
        let Some(gpu) = self.gpu.as_ref() else { return (0, 0) };
        let runs = gpu.text_runs.borrow();
        let markiert = self.selected_ranges(&runs);
        let gesamt = markiert.iter().map(|(i, _)| runs[*i].glyphs.len()).sum();
        (selection_text(&runs, &markiert).chars().count(), gesamt)
    }

    pub(crate) fn selected_chars(&mut self) -> usize {
        self.selection_extent().0
    }

    // Strg+D: jede Markierung aufheben (Eingabefeld, Chat, Log). Gibt zurueck, ob es eine gab --
    // sonst laeuft die Taste normal weiter.
    pub(crate) fn deselect_all(&mut self) -> bool {
        if self.selected_chars() == 0 {
            return false;
        }
        if let Some(feld) = self.active_text_field() {
            feld.deselect();
        }
        if let Some(c) = self.chat_input.as_mut() {
            c.deselect();
        }
        self.text_selection = None;
        self.selection_sound_deselect = true;
        true
    }

    // Wo die Markierung der Zeile von `ziel` in diesem Bild steht: sie gleitet in CARET_GLIDE
    // nach `ziel`, von dort, wo sie gerade war -- oder, neu in dieser Zeile, von `anfang`. Dazu
    // die Staerke des Scheins an ihrer linken und rechten Kante: an jeder, die sich mit der
    // letzten Aenderung bewegt hat, voll, solange sie gleitet, danach klingt er in SELECTION_FADE
    // ab.
    pub(crate) fn glide_selection(&self, ziel: Rect, anfang: Rect) -> (Rect, (f32, f32)) {
        let jetzt = Instant::now();
        let gleich = |a: Rect, b: Rect| a.x == b.x && a.y == b.y && a.w == b.w && a.h == b.h;
        let jetzt_bei = |g: &SelectionGlide| mix_rect(g.from, g.to, ease_out(jetzt.duration_since(g.start).as_secs_f32() / CARET_GLIDE.as_secs_f32()));
        let mut zeilen = self.selection_glide.borrow_mut();
        if zeilen.len() > 64 {
            zeilen.retain(|_, g| jetzt.duration_since(g.seen) < Duration::from_secs(1));
        }
        let glide = match zeilen.get(&(ziel.y.round() as i32)).copied() {
            Some(g) if jetzt.duration_since(g.seen) < Duration::from_millis(100) && g.to.h == ziel.h => {
                if gleich(g.to, ziel) { g } else { SelectionGlide { from: jetzt_bei(&g), to: ziel, start: jetzt, seen: jetzt } }
            }
            _ => SelectionGlide { from: anfang, to: ziel, start: jetzt, seen: jetzt },
        };
        zeilen.insert(ziel.y.round() as i32, SelectionGlide { seen: jetzt, ..glide });
        let danach = jetzt.duration_since(glide.start).saturating_sub(CARET_GLIDE);
        let glow = 1.0 - ease_out(danach.as_secs_f32() / SELECTION_FADE.as_secs_f32());
        let links = (glide.from.x - glide.to.x).abs() > 0.5;
        let rechts = (glide.from.x + glide.from.w - glide.to.x - glide.to.w).abs() > 0.5;
        (jetzt_bei(&glide), (if links { glow } else { 0.0 }, if rechts { glow } else { 0.0 }))
    }

    // Wie weit der Knopf mit der Flaeche `r` gerade gehovert ist (s. HoverAnim, HOVER_FLASH):
    // `hover` ist, ob der Zeiger jetzt darauf steht. Der Stand haengt an der Flaeche -- dieselbe
    // Flaeche im naechsten Bild ist derselbe Knopf.
    pub(crate) fn hover_level(&self, r: Rect, hover: bool) -> f32 {
        let jetzt = Instant::now();
        let key = (r.x.round() as i32, r.y.round() as i32, r.w.round() as i32, r.h.round() as i32);
        let mut anims = self.hover_anims.borrow_mut();
        // Laenger nicht gezeichnete Knoepfe vergessen (Seitenwechsel, Scrollen).
        if anims.len() > 256 {
            anims.retain(|_, a| jetzt.duration_since(a.seen) < Duration::from_secs(1));
        }
        let a = anims.entry(key).or_insert(HoverAnim { hovered: false, since: jetzt, from: 0.0, seen: jetzt });
        // Lange nicht gezeichnet: kein Nachleuchten von damals.
        if jetzt.duration_since(a.seen) > Duration::from_millis(100) {
            *a = HoverAnim { hovered: false, since: jetzt, from: 0.0, seen: jetzt };
        }
        if a.hovered != hover {
            *a = HoverAnim { hovered: hover, since: jetzt, from: a.level(jetzt), seen: jetzt };
        }
        a.seen = jetzt;
        if hover {
            self.hover_drawn.set(Some((key, jetzt)));
        } else if self.hover_drawn.get().is_some_and(|(k, _)| k == key) {
            self.hover_drawn.set(None);
        }
        a.level(jetzt)
    }

    // Schreibcursor zeichnen: `ziel` ist, wo er jetzt hingehoert. Eine Stelle fuer alle
    // Eingabefelder (Chat, Wert-, Namens-, Adress- und Suchfelder): sie gleiten (s. CARET_GLIDE)
    // und blinken gleich -- sichtbar ab der letzten Bewegung, dann halb an, halb aus.
    pub(crate) fn push_caret(&self, rects: &mut Vec<UiRect>, ziel: Rect) {
        let jetzt = Instant::now();
        self.caret_drawn.set(Some((ziel.x, ziel.y)));
        let gleich = |a: &Rect| a.y == ziel.y && a.h == ziel.h;
        let a = match self.caret_anim.get() {
            Some(a) if gleich(&a.to) && a.to.x == ziel.x => a,
            Some(a) if gleich(&a.to) && jetzt.duration_since(a.seen) < Duration::from_millis(100) => CaretAnim { from_x: a.x(jetzt), to: ziel, start: jetzt, seen: jetzt },
            _ => CaretAnim { from_x: ziel.x, to: ziel, start: jetzt, seen: jetzt },
        };
        self.caret_anim.set(Some(CaretAnim { seen: jetzt, ..a }));
        let [r, g, b, alpha] = theme::accent_2();
        let alpha = alpha * caret_alpha(jetzt.duration_since(a.start));
        rects.push(UiRect { rect: Rect { x: a.x(jetzt), ..ziel }, color: [r, g, b, alpha], ..UiRect::FLAT });
    }

    // Pfeil oben/unten im Chat. Wechselt dabei der Verlauf den Text (nicht die Vorschlagsliste),
    // gleiten alter und neuer Text uebereinander (s. ChatRecall) -- und nicht seitlich wie beim
    // Tippen.
    pub(crate) fn step_chat(&mut self, step: chat_input::Step) {
        let Some(feld) = self.chat_input.as_mut() else { return };
        let vorschlaege = feld.has_suggestions();
        let alt = feld.text().to_string();
        feld.step(step);
        // Ein Schritt in der Vorschlagsliste klickt, eine Nachricht aus dem Verlauf ploppt auf;
        // am Ende des Verlaufs passiert nichts und es bleibt still.
        if vorschlaege {
            self.play_ui_sound(ui_sound::UiSound::Step);
        } else if feld.text() != alt {
            self.chat_recall = Some(ChatRecall { alt, nach_oben: step == chat_input::Step::Up, start: Instant::now() });
            *self.chat_slide.borrow_mut() = None;
            *self.chat_gap.borrow_mut() = None;
            self.play_ui_sound(ui_sound::UiSound::Popup);
        }
    }

    // Chat: wie weit der Text links vom Cursor gerade versetzt steht (s. ChatSlide). `line` ist
    // die Zeile des Cursors, `line_x` wo sie jetzt beginnt.
    pub(crate) fn chat_slide_offset(&self, text: &str, line: usize, line_x: f32) -> f32 {
        let jetzt = Instant::now();
        let mut s = self.chat_slide.borrow_mut();
        match s.as_mut() {
            Some(alt) if alt.text != text => {
                // Getippt oder geloescht: dort weitermachen, wo der Text gerade steht.
                let from = if alt.line == line { alt.offset(jetzt) + alt.line_x - line_x } else { 0.0 };
                *alt = ChatSlide { text: text.to_string(), line, line_x, from, start: jetzt };
            }
            Some(alt) => {
                alt.line = line;
                alt.line_x = line_x;
            }
            None => *s = Some(ChatSlide { text: text.to_string(), line, line_x, from: 0.0, start: jetzt }),
        }
        s.as_ref().map_or(0.0, |a| a.offset(jetzt))
    }

    // Der eigene Spielername, wie ihn der Server kennt.
    pub(crate) fn own_name(&self) -> Option<&str> {
        self.own_uuid.and_then(|u| self.player_names.get(&u)).or(self.credentials.as_ref().map(|c| &c.username)).map(String::as_str)
    }

    // Abschnitte mit umgeschriebenem/markiertem eigenen Namen; None = nichts zu tun.
    pub(crate) fn ego_rewrite(&self, spans: &[protocol::nbt::TextSpan]) -> Option<Vec<protocol::nbt::TextSpan>> {
        let set = &self.settings;
        if !set.module_ego_name {
            return None;
        }
        let r = ego_name::Rewrite { name: self.own_name()?, alias: &set.ego_alias, deco: set.ego_deco != ego_name::Deco::Off, shadow: set.ego_shadow != ego_name::Deco::Off };
        ego_name::rewrite(spans, &r)
    }

    // Wie ego_rewrite, fuer Zeichenwege, die die Abschnitte nur ausleihen.
    pub(crate) fn ego_spans<'a>(&self, spans: &'a [protocol::nbt::TextSpan]) -> std::borrow::Cow<'a, [protocol::nbt::TextSpan]> {
        match self.ego_rewrite(spans) {
            Some(neu) => std::borrow::Cow::Owned(neu),
            None => std::borrow::Cow::Borrowed(spans),
        }
    }

    // Eine Nachricht vom Server: Name umschreiben, und nennt sie ihn (nicht nur als Absender),
    // klingt der Erwaehnungston.
    pub(crate) fn ego_incoming(&mut self, spans: Vec<protocol::nbt::TextSpan>, ton: bool) -> Vec<protocol::nbt::TextSpan> {
        if !self.settings.module_ego_name {
            return spans;
        }
        if ton && self.own_name().is_some_and(|n| ego_name::mentions(&spans, n)) {
            self.play_ego_sound();
        }
        self.ego_rewrite(&spans).unwrap_or(spans)
    }

    // Stand fuer paint()/shadow(), einmal je Durchlauf der Ereignisschleife.
    pub(crate) fn publish_ego_look(&self) {
        let set = &self.settings;
        let look = set.module_ego_name.then(|| ego_name::Look {
            name: ego_name::Paint { deco: set.ego_deco, a: set.ego_rgb(0), b: set.ego_rgb(1), light: set.ego_light as f32 / 100.0 },
            speed: set.ego_speed as f32 / 10.0,
            shadow: ego_name::Paint { deco: set.ego_shadow, a: set.ego_rgb(2), b: set.ego_rgb(3), light: set.ego_shadow_light as f32 / 100.0 },
            shadow_alpha: set.ego_colors[2][3] as f32 / 100.0,
        });
        ego_name::set_look(look);
    }

    pub(crate) fn push_chat_message(&mut self, spans: Vec<protocol::nbt::TextSpan>) {
        // No chat clear: Clear-Chat-Plugins leeren den Chat mit Dutzenden Leerzeilen; leere
        // Nachrichten kommen dann gar nicht erst hinein.
        if self.settings.no_chat_clear && spans.iter().all(|s| s.text.trim().is_empty()) {
            return;
        }
        chat_log::append(&spans.iter().map(|s| s.text.as_str()).collect::<String>());
        let spans = linkify_chat(spans);
        // Der geschlossene Chat rueckt um die Zeilen der neuen Nachricht hoch, gleitend (s.
        // chat_rise). Gedeckelt, damit eine Flut nicht ewig nachlaeuft.
        if let Some(gpu) = self.gpu.as_ref() {
            let _schrift = self.area_font_scope(gpu, ShadowArea::Chat);
            let scale = self.chat_scale();
            let breite = self.hud_chat_rect(gpu).w - 4.0;
            let n = wrap_chat_message(&spans, breite, |t| gpu.measure_text(t, scale)).len();
            self.chat_rise = (self.chat_rise + n as f32).min(self.settings.chat_lines as f32);
            self.chat_lines_total += n as u64;
            let breite = self.chat_open_rect(gpu).w - 12.0;
            let n = wrap_chat_message(&spans, breite, |t| gpu.measure_text(t, scale)).len();
            self.chat_rise_open = (self.chat_rise_open + n as f32).min(self.settings.chat_lines_open as f32);
        }
        self.chat_messages.push_back(spans);
        self.chat_times.push_back(Instant::now());
        if self.chat_messages.len() > CHAT_MAX_MESSAGES {
            self.chat_times.pop_front();
            if let Some(alt) = self.chat_messages.pop_front() {
                // Mitzaehlen, wie viele Zeilen oben wegfallen -- sonst saehe es fuer die
                // Auswahl so aus, als ruecke der Chat nicht weiter (s. chat_view).
                if let Some(gpu) = self.gpu.as_ref() {
                    let _schrift = self.area_font_scope(gpu, ShadowArea::Chat);
                    let scale = self.chat_scale();
                    let breite = self.chat_open_rect(gpu).w - 12.0;
                    self.chat_dropped_lines += wrap_chat_message(&alt, breite, |t| gpu.measure_text(t, scale)).len();
                }
            }
        }
        self.chat_generation += 1;
        // Beim Zurueckblaettern bleibt die Ansicht stehen, statt von neuen Nachrichten
        // weggeschoben zu werden (Vanilla: ChatHud.addVisibleMessage).
        if self.chat_scroll > 0 {
            self.chat_scroll += 1;
            // Auch der gleitende Stand, sonst glitte die Ansicht eine Zeile nach.
            self.chat_scroll_glide.ziel += 1.0;
            self.chat_scroll_glide.stand += 1.0;
        }
    }

    // --- Textauswahl und Zwischenablage -----------------------------------------------

    pub(crate) fn selection_context(&self) -> SelectionContext {
        (self.mode, self.screen, self.chat_input.is_some(), self.module_menu_open)
    }

    // Wo man Text markieren kann: ueberall, wo es einen freien Zeiger gibt -- in den Menues,
    // im offenen Chat und im Modul-Menue. NICHT im Inventar (dort zieht die Maus Stapel) und
    // nicht beim Verschieben von HUD-Elementen.
    pub(crate) fn selection_allowed(&self) -> bool {
        match self.mode {
            AppMode::Menu => self.screen != MenuScreen::HudEditor,
            AppMode::World => !self.inventory.screen_open && (self.chat_input.is_some() || self.module_menu_open),
        }
    }

    // Wo ein Ziehen mit der Maus Text markiert: nur dort, wo Text zum Kopieren steht -- im
    // offenen Chat. Beschriftungen in Einstellungen, Server-Liste und Modul-Menue bleiben
    // unmarkierbar, dort ist Ziehen nur Klicken.
    pub(crate) fn selection_start_allowed(&self) -> bool {
        self.selection_allowed() && self.mode == AppMode::World && self.chat_input.is_some()
    }

    // Die aktuelle Auswahl als (Zeile, Zeichenbereich) -- leer, wenn es keine gibt oder sie
    // zu einem anderen Bildschirm gehoert.
    pub(crate) fn selected_ranges(&self, runs: &[TextRun]) -> Vec<(usize, std::ops::Range<usize>)> {
        let Some(sel) = &self.text_selection else { return Vec::new() };
        let dy = self.selection_shift(sel);
        let (anchor, focus) = ((sel.anchor.0, sel.anchor.1 + dy), (sel.focus.0, sel.focus.1 + dy));
        match sel {
            _ if sel.context != self.selection_context() => Vec::new(),
            _ if sel.clicks >= 2 => select_in_runs_by_unit(runs, anchor, focus, sel.clicks),
            _ if sel.moved => select_in_runs(runs, anchor, focus),
            _ => Vec::new(),
        }
    }

    // Um wie viele Pixel der Chattext seit dem Beginn der Auswahl nach oben (negativ) oder
    // unten gerueckt ist -- gespeicherte Punkte + das = wo derselbe Text jetzt steht.
    pub(crate) fn selection_shift(&self, sel: &TextSelection) -> f32 {
        match (sel.chat_anchor, self.chat_view.get()) {
            (Some((damals, line_h)), Some((jetzt, _))) => (damals as f32 - jetzt as f32) * line_h,
            _ => 0.0,
        }
    }

    pub(crate) fn selected_text(&self) -> String {
        let Some(gpu) = self.gpu.as_ref() else { return String::new() };
        let runs = gpu.text_runs.borrow();
        let sel = self.selected_ranges(&runs);
        selection_text(&runs, &sel)
    }

    // Die Markierung hinter dem ausgewaehlten Text (vom letzten Bild -- der Text selbst
    // entsteht erst beim Zeichnen). Kommt ans ENDE der Rechteckliste: ueber Panels und
    // Knoepfe, aber wie alle Flaechen unter dem Text.
    pub(crate) fn push_selection_rects(&self, gpu: &GpuState, rects: &mut Vec<UiRect>) {
        let runs = gpu.text_runs.borrow();
        // Je Zeile EINE Flaeche: getippter Text besteht aus vielen kleinen Laeufen (ein Zeichen
        // je Lauf), die sonst einzeln aufleuchten wuerden.
        let mut zeilen: Vec<Rect> = Vec::new();
        for (i, r) in self.selected_ranges(&runs) {
            let run = &runs[i];
            let (x0, x1) = (run.glyphs[r.start].1, run.glyphs[r.end - 1].2);
            match zeilen.iter_mut().find(|z| z.y == run.top && z.h == run.bottom - run.top) {
                Some(z) => {
                    let rechts = (z.x + z.w).max(x1);
                    z.x = z.x.min(x0);
                    z.w = rechts - z.x;
                }
                None => zeilen.push(Rect { x: x0, y: run.top, w: x1 - x0, h: run.bottom - run.top }),
            }
        }
        for ziel in zeilen {
            // Neu in einer Zeile waechst die Markierung vom Anker weg: in seiner Zeile von ihm aus
            // (Doppelklick mitten ins Wort: in beide Richtungen), darunter von links, darueber
            // von rechts.
            let start_x = self.text_selection.as_ref().map_or(ziel.x, |s| {
                let (ax, ay) = (s.anchor.0, s.anchor.1 + self.selection_shift(s));
                if ay < ziel.y { ziel.x } else if ay >= ziel.y + ziel.h { ziel.x + ziel.w } else { ax.clamp(ziel.x, ziel.x + ziel.w) }
            });
            let (f, glow) = self.glide_selection(ziel, Rect { x: start_x, w: 0.0, ..ziel });
            self.push_selection_span(rects, f, glow);
        }
    }

    // Welcher Teil des Chat-EINGABEtextes markiert ist, als Byte-Bereich in feld.text().
    // Die Auswahl kennt nur gezeichnete Zeichen (vom letzten Bild); hier wird sie auf die
    // Zeilen des Eingabefeldes zurueckgerechnet -- gleiche Lage wie in build_chat_overlay.
    pub(crate) fn chat_input_selection(&self) -> Option<std::ops::Range<usize>> {
        let gpu = self.gpu.as_ref()?;
        let _schrift = self.area_font_scope(gpu, ShadowArea::Chat);
        let feld = self.chat_input.as_ref()?;
        let text = feld.text();
        let runs = gpu.text_runs.borrow();
        let markiert = self.selected_ranges(&runs);
        if markiert.is_empty() {
            return None;
        }
        let rect = self.chat_input_rect(gpu);
        let line_h = Self::chat_input_line_h(gpu);
        let mass = |t: &str| gpu.measure_raw(t, UI_SCALE_NORMAL);
        let mut bereich: Option<std::ops::Range<usize>> = None;
        for (i, zeile) in self.chat_input_lines(gpu, text).iter().enumerate() {
            let y = rect.y + 3.0 + i as f32 * line_h + 1.0;
            let x = (self.chat_line_end(gpu, text, i) - mass(zeile)).round();
            let versatz = zeile.as_ptr() as usize - text.as_ptr() as usize;
            for (run_i, r) in &markiert {
                let run = &runs[*run_i];
                let passt = (run.top - y).abs() < 0.5 && run.glyphs.first().is_some_and(|g| (g.1 - x).abs() < 0.5);
                if !passt || r.is_empty() {
                    continue;
                }
                let (gx0, gx1) = (run.glyphs[r.start].1, run.glyphs[r.end - 1].2);
                for (b, ch) in zeile.char_indices() {
                    let mitte = x + (mass(&zeile[..b]) + mass(&zeile[..b + ch.len_utf8()])) / 2.0;
                    if mitte >= gx0 && mitte <= gx1 {
                        let (a, e) = (versatz + b, versatz + b + ch.len_utf8());
                        bereich = Some(match bereich {
                            Some(alt) => alt.start.min(a)..alt.end.max(e),
                            None => a..e,
                        });
                    }
                }
            }
        }
        bereich
    }

    // Tippen/Einfuegen/Loeschen in die Chat-Eingabe: ist dort Text markiert, wird ER ersetzt
    // (bzw. mit leerem `text` geloescht) und die Markierung verschwindet. Ohne Markierung:
    // anhaengen, bzw. bei `loeschen` das letzte Zeichen. true = der Text hat sich geaendert.
    pub(crate) fn chat_input_edit(&mut self, text: &str, loeschen: bool) -> bool {
        let markiert = self.chat_input.as_ref().and_then(chat_input::ChatInput::keyboard_selection).or_else(|| self.chat_input_selection());
        let Some(feld) = self.chat_input.as_mut() else { return false };
        let geaendert = match markiert {
            Some(bereich) => {
                self.text_selection = None;
                feld.replace_range(bereich, text)
            }
            None if loeschen => feld.backspace(),
            None => feld.push_str(text),
        };
        if geaendert {
            self.play_ui_sound(if loeschen { ui_sound::UiSound::Erase } else { ui_sound::UiSound::Type });
            self.request_command_suggestions();
        }
        geaendert
    }

    // Strg+A im Chat: die ganze Eingabe markieren (erste bis letzte Zeile).
    pub(crate) fn chat_select_all(&mut self) {
        if let Some(feld) = self.chat_input.as_mut() {
            feld.select_all();
        }
        self.chat_show_keyboard_selection();
    }

    // Cursor im Eingabefeld bewegen (s. chat_input::Move). Ohne Shift springt er bei einer
    // Markierung an deren Rand, wie in Vanilla; Vorschlaege gelten fuer den Text bis zum
    // Cursor und werden neu angefragt.
    pub(crate) fn chat_move_cursor(&mut self, m: chat_input::Move, shift: bool) {
        let markiert = self.chat_input.as_ref().and_then(chat_input::ChatInput::keyboard_selection).or_else(|| self.chat_input_selection());
        let Some(feld) = self.chat_input.as_mut() else { return };
        let bewegt = match markiert {
            Some(r) if !shift && matches!(m, chat_input::Move::Left | chat_input::Move::Right) => {
                feld.set_cursor(if m == chat_input::Move::Left { r.start } else { r.end });
                true
            }
            _ => feld.move_cursor(m, shift),
        };
        // Eine Mausmarkierung endet; eine Tastaturmarkierung wird neu gesetzt (auch leer).
        self.text_selection = None;
        self.chat_show_keyboard_selection();
        if bewegt {
            self.play_ui_sound(ui_sound::UiSound::Step);
            self.request_command_suggestions();
        }
    }

    // Ein blosser Klick in die Chat-Eingabe: den Cursor an die naechste Zeichengrenze setzen.
    pub(crate) fn chat_click_cursor(&mut self, pos: (f32, f32)) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let Some(feld) = self.chat_input.as_ref() else { return };
        let rect = self.chat_input_rect(gpu);
        if !rect.contains(pos.0, pos.1) {
            return;
        }
        let _schrift = self.area_font_scope(gpu, ShadowArea::Chat);
        let text = feld.text();
        let zeilen = self.chat_input_lines(gpu, text);
        let Some(letzte) = zeilen.len().checked_sub(1) else { return };
        let line_h = Self::chat_input_line_h(gpu);
        let i = (((pos.1 - rect.y - 3.0) / line_h).floor().max(0.0) as usize).min(letzte);
        let zeile = zeilen[i];
        let start = zeile.as_ptr() as usize - text.as_ptr() as usize;
        let caret_x = self.chat_line_end(gpu, text, i);
        let ziel = zeile
            .char_indices()
            .map(|(k, _)| k)
            .chain([zeile.len()])
            .min_by(|&a, &b| {
                let x = |k: usize| (caret_x - gpu.measure_raw(&zeile[k..], UI_SCALE_NORMAL) - pos.0).abs();
                x(a).total_cmp(&x(b))
            })
            .map_or(start, |k| start + k);
        drop(_schrift);
        let Some(feld) = self.chat_input.as_mut() else { return };
        let vorher = (feld.cursor(), feld.keyboard_selection().is_some());
        feld.set_cursor(ziel);
        if vorher != (ziel, false) {
            if !vorher.1 {
                self.play_ui_sound(ui_sound::UiSound::Slider);
            }
            self.request_command_suggestions();
        }
    }

    // Wo die Stelle `b` (Byte in `text`) im Eingabefeld steht: (Zeile, x der Zeichengrenze).
    // `davor`: an einer Umbruchstelle das Ende der vorigen Zeile statt des Anfangs der
    // naechsten (fuer das Ende einer Markierung).
    pub(crate) fn chat_input_spot(&self, gpu: &GpuState, text: &str, b: usize, davor: bool) -> (usize, f32) {
        let zeilen = self.chat_input_lines(gpu, text);
        let bereich = |z: &str| {
            let start = z.as_ptr() as usize - text.as_ptr() as usize;
            (start, start + z.len())
        };
        let i = zeilen
            .iter()
            .position(|z| {
                let (s, e) = bereich(z);
                if davor { b > s && b <= e } else { b >= s && b < e }
            })
            .unwrap_or(if b == 0 { 0 } else { zeilen.len() - 1 });
        let (s, e) = bereich(zeilen[i]);
        let rest = &zeilen[i][b.clamp(s, e) - s..];
        (i, self.chat_line_end(gpu, text, i) - gpu.measure_raw(rest, UI_SCALE_NORMAL))
    }

    // Die Tastatur-Markierung (Shift+Pfeil, Strg+A) als gewoehnliche Textauswahl: dieselbe
    // Hervorhebung und dasselbe Kopieren wie eine mit der Maus gezogene.
    pub(crate) fn chat_show_keyboard_selection(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let _schrift = self.area_font_scope(gpu, ShadowArea::Chat);
        let Some(feld) = self.chat_input.as_ref() else { return };
        let Some(r) = feld.keyboard_selection() else { return };
        let rect = self.chat_input_rect(gpu);
        let line_h = Self::chat_input_line_h(gpu);
        let oben = rect.y + 3.0 + 1.0 + line_h / 2.0;
        let (za, xa) = self.chat_input_spot(gpu, feld.text(), r.start, false);
        let (ze, xe) = self.chat_input_spot(gpu, feld.text(), r.end, true);
        let selection = TextSelection {
            anchor: (xa.round() + 0.5, oben + za as f32 * line_h),
            focus: (xe.round() - 0.5, oben + ze as f32 * line_h),
            dragging: false,
            moved: true,
            clicks: 1,
            context: self.selection_context(),
            chat_anchor: None,
        };
        self.text_selection = Some(selection);
    }

    pub(crate) fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }

    // Linke Taste gedrueckt: eine neue Auswahl beginnt (und eine alte endet).
    // Kurz nacheinander an derselben Stelle: Doppelklick waehlt das Wort, Dreifachklick die
    // Zeile (weitere Klicks bleiben bei der Zeile).
    pub(crate) fn selection_press(&mut self) {
        let jetzt = Instant::now();
        let pos = self.cursor_pos;
        let clicks = match self.last_left_click {
            Some((t, p, n)) if jetzt.duration_since(t).as_millis() <= MULTI_CLICK_MS && (pos.0 - p.0).abs() + (pos.1 - p.1).abs() <= MULTI_CLICK_PX => (n + 1).min(3),
            _ => 1,
        };
        self.last_left_click = Some((jetzt, pos, clicks));
        self.selection_sound_deselect = true;
        self.selection_sound_hint = match clicks {
            2 => Some(ui_sound::UiSound::Step),
            3 => Some(ui_sound::UiSound::Click),
            _ => None,
        };
        // Im offenen Chatverlauf gedrueckt? Dann haengt die Auswahl am Text (s. chat_anchor).
        let im_verlauf = self.chat_input.is_some() && self.gpu.as_ref().is_some_and(|gpu| self.chat_open_rect(gpu).contains(pos.0, pos.1));
        self.text_selection = self.selection_start_allowed().then(|| TextSelection {
            anchor: pos,
            focus: pos,
            dragging: true,
            moved: false,
            clicks,
            context: self.selection_context(),
            chat_anchor: if im_verlauf { self.chat_view.get() } else { None },
        });
    }

    // Zieht man beim Markieren unter (oder ueber) den offenen Chatverlauf hinaus, blaettert
    // er mit -- so wie in jedem Textfeld.
    //
    // Gehoert ins Bild und nicht in die Mausbewegung: wer den Zeiger unten HAELT, ohne ihn
    // weiterzubewegen, soll trotzdem weiterblaettern.
    pub(crate) fn chat_selection_autoscroll(&mut self) {
        // Nur waehrend eines Ziehens, das im Chatverlauf begonnen hat (chat_anchor).
        let zieht = self.text_selection.as_ref().is_some_and(|s| s.dragging && s.chat_anchor.is_some());
        if !zieht || self.chat_input.is_none() {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let rect = self.chat_open_draw_rect(gpu);
        let y = self.cursor_pos.1;
        // Abstand zur naechsten Kante, in Zeilenhoehen. Je weiter draussen, desto schneller --
        // aber gedeckelt, sonst rauscht ein weiter Zug durch den ganzen Verlauf.
        let line_h = self.chat_line_height(gpu).max(1.0);
        let heraus = if y > rect.y + rect.h {
            y - (rect.y + rect.h)
        } else if y < rect.y {
            y - rect.y
        } else {
            return;
        };
        let zeilen = (heraus / line_h).clamp(-CHAT_AUTOSCROLL_MAX_LINES, CHAT_AUTOSCROLL_MAX_LINES);
        // scroll_chat blaettert bei positivem Wert in die VERGANGENHEIT (nach oben), der
        // Zeiger unterhalb des Chats soll aber nach unten laufen -- daher das Minus.
        self.scroll_chat(-zeilen);
        // Der Griffpunkt der Auswahl wandert mit dem Text mit (s. selection_shift), also den
        // Fokus im selben Bild nachziehen.
        self.selection_drag();
    }

    pub(crate) fn selection_drag(&mut self) {
        let pos = self.cursor_pos;
        let dy = self.text_selection.as_ref().map_or(0.0, |sel| self.selection_shift(sel));
        if let Some(sel) = self.text_selection.as_mut().filter(|s| s.dragging) {
            // Im Stand vom Druecken speichern (s. selection_shift), sonst sprang das Ende beim
            // Ziehen waehrend einer neuen Nachricht um eine Zeile.
            let pos = (pos.0, pos.1 - dy);
            sel.focus = pos;
            if (pos.0 - sel.anchor.0).abs() + (pos.1 - sel.anchor.1).abs() >= SELECTION_DRAG_MIN {
                sel.moved = true;
            }
        }
    }

    // Linke Taste los: ein blosser Klick hebt die Auswahl auf, ein Ziehen legt den Text in
    // die PRIMAERE Auswahl -- wie ueberall unter Linux: markieren genuegt, Mittelklick fuegt
    // ein (s. paste_primary).
    pub(crate) fn selection_release(&mut self) {
        let Some(sel) = self.text_selection.as_mut() else { return };
        sel.dragging = false;
        if !sel.moved && sel.clicks < 2 {
            let klick = sel.anchor;
            self.text_selection = None;
            self.chat_click_cursor(klick);
            return;
        }
        let text = self.selected_text();
        if text.is_empty() {
            return;
        }
        // Die primaere Auswahl gibt es nur unter X11/Wayland -- Windows kennt sie nicht.
        #[cfg(target_os = "linux")]
        if let Some(cb) = self.clipboard() {
            use arboard::SetExtLinux;
            let _ = cb.set().clipboard(arboard::LinuxClipboardKind::Primary).text(text);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = text;
    }

    // Strg+C: die Auswahl in die normale Zwischenablage. false = es gab nichts zu kopieren.
    pub(crate) fn copy_selection(&mut self) -> bool {
        let text = self.selected_text();
        if text.is_empty() {
            return false;
        }
        let n = text.chars().count();
        if let Some(cb) = self.clipboard() {
            let _ = cb.set_text(text);
        }
        self.push_log(format!("{n} Zeichen kopiert"));
        self.notify(notifications::Notification::info("In die Zwischenablage kopiert", format!("{n} Zeichen")));
        true
    }

    // Mittelklick: die primaere Auswahl (irgendwo markierter Text, auch in anderen Programmen)
    // ins gerade aktive Eingabefeld.
    pub(crate) fn paste_primary(&mut self) {
        #[cfg(target_os = "linux")]
        let text = self.clipboard().and_then(|cb| {
            use arboard::GetExtLinux;
            cb.get().clipboard(arboard::LinuxClipboardKind::Primary).text().ok()
        });
        #[cfg(not(target_os = "linux"))]
        let text: Option<String> = None;
        if let Some(text) = text {
            self.paste_into_focus(&text);
        }
    }

    // Text ins aktive Eingabefeld -- dieselbe Vorrangreihenfolge wie beim Tippen: Chat, dann
    // Server-Formular, dann Token-/Client-ID-Feld, dann die Server-Adresse. true = es gab ein
    // Ziel.
    pub(crate) fn paste_into_focus(&mut self, text: &str) -> bool {
        if self.chat_input.is_some() {
            if self.chat_input.as_mut().map(|c| c.push_str(text)).unwrap_or(false) {
                self.play_ui_sound(ui_sound::UiSound::Type);
                self.request_command_suggestions();
            }
            return true;
        }
        if self.theme_name_dialog.is_some() {
            self.server_edit_insert(text);
            return true;
        }
        if self.mode != AppMode::Menu {
            return false;
        }
        if self.server_edit.is_some() || self.quick_typing() {
            self.server_edit_insert(text);
            return true;
        }
        if let Some((_, buf)) = &mut self.input_field {
            buf.extend(text.chars().filter(|c| !c.is_control()));
            return true;
        }
        if self.screen == MenuScreen::SettingsLightLevels {
            self.light_search.extend(text.chars().filter(|c| !c.is_control()));
            self.light_scroll = Default::default();
            return true;
        }
        false
    }

    // Mausrad im offenen Chat: `lines` > 0 blaettert zurueck in die Vergangenheit.
    pub(crate) fn scroll_chat(&mut self, lines: f32) {
        let schritte = lines.round() as i64;
        let gesamt = self.chat_wrap_cache.borrow().as_ref().map(|(_, l)| l.len()).unwrap_or(0);
        // Das GRUNDrechteck, wie in build_chat_overlay_body: das Wachsen nach unten kostet
        // keine eigenen Raststufen, es verbraucht dieselben (s. dortigen Kommentar).
        let sichtbar = self
            .gpu
            .as_ref()
            .map(|gpu| {
                let _schrift = self.area_font_scope(gpu, ShadowArea::Chat);
                (self.chat_open_rect(gpu).h / self.chat_line_height(gpu)).round() as usize
            })
            .unwrap_or(1);
        let max = gesamt.saturating_sub(sichtbar) as i64;
        self.chat_scroll = (self.chat_scroll as i64 + schritte).clamp(0, max) as usize;
    }

    // Mausrad ueber der Verlaufsliste: `lines` > 0 ist das Rad nach oben (zu neueren
    // Eintraegen, die oben stehen), nach unten geht es zu aelteren.
    pub(crate) fn scroll_chat_history(&mut self, lines: f32) {
        let lines = -lines;
        let gesamt = self.chat_input.as_ref().map_or(0, |f| f.history().len());
        let platz = self
            .gpu
            .as_ref()
            .map(|gpu| (self.chat_history_rect(gpu).h / (gpu.text_height(UI_SCALE_SMALL) + 4.0)).floor() as usize)
            .unwrap_or(1);
        let max = gesamt.saturating_sub(platz) as i64;
        self.chat_history_scroll = (self.chat_history_scroll as i64 + lines.round() as i64).clamp(0, max) as usize;
    }

    // Klick in den offenen Chat: ein Verlaufseintrag kommt in die Eingabe; ein Link geht ueber
    // open_link (nur http(s), unbekannte Seiten mit Rueckfrage). Die uebrigen Klick-Aktionen wie Vanillas ChatScreen.handleClickEvent:
    // Shift+Klick fuegt Style.insertion ein, sonst run_command / suggest_command /
    // copy_to_clipboard.
    pub(crate) fn chat_click(&mut self, px: f32, py: f32) {
        let eintrag = self.chat_history_hits.borrow().iter().find(|(r, _)| r.contains(px, py)).map(|(_, t)| t.clone());
        if let Some(eintrag) = eintrag {
            if let Some(feld) = self.chat_input.as_mut() {
                feld.set_text(&eintrag);
            }
            self.play_ui_sound(ui_sound::UiSound::Type);
            self.request_command_suggestions();
            return;
        }
        let treffer = self.chat_link_hits.borrow().iter().find(|(r, _)| r.contains(px, py)).map(|(_, h)| h.clone());
        let Some(treffer) = treffer else { return };
        if self.modifiers.shift_key() {
            // Mit Shift zaehlt NUR die Einfuegung, auch wenn es keine gibt (Vanilla: `if (insert)
            // { if (insertion != null) ... }` ohne else).
            if let Some(text) = treffer.action.as_ref().and_then(|a| a.insertion.clone()) {
                if let Some(feld) = self.chat_input.as_mut() {
                    feld.push_str(&text);
                }
                self.request_command_suggestions();
            }
            return;
        }
        if let Some(klick) = treffer.action.as_ref().and_then(|a| a.click.clone()) {
            self.run_chat_click(klick);
            return;
        }
        if let Some(url) = treffer.link {
            self.open_link(url);
        }
    }

    pub(crate) fn run_chat_click(&mut self, klick: protocol::nbt::ClickAction) {
        use protocol::nbt::ClickAction;
        match klick {
            // Vanilla (ClientPlayNetworkHandler.runClickEventCommand) schickt den Befehl ohne
            // Nachfrage als CommandExecution, wenn er sich sauber und ohne signierte Argumente
            // parsen laesst; sonst fragt es nach oder legt ihn nur in die Eingabe. Den
            // Befehlsbaum wertet dieser Client nicht aus -- er schickt ihn wie einen getippten
            // Befehl, die Pruefung macht der Server ohnehin. Der Chat bleibt offen, wie in
            // Vanilla (screenAfterRun ist der Chat selbst).
            ClickAction::RunCommand(befehl) => {
                let befehl = befehl.strip_prefix('/').unwrap_or(&befehl).to_string();
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::chat_command_payload(&befehl));
                }
                self.push_log(format!("Befehl aus dem Chat: /{befehl}"));
            }
            // insertText(command, override = true): ersetzt die ganze Eingabe.
            ClickAction::SuggestCommand(befehl) => {
                if let Some(feld) = self.chat_input.as_mut() {
                    feld.set_text(&befehl);
                }
                self.play_ui_sound(ui_sound::UiSound::Type);
                self.request_command_suggestions();
            }
            ClickAction::CopyToClipboard(text) => {
                if let Some(cb) = self.clipboard() {
                    let _ = cb.set_text(text);
                }
            }
            // Screen.defaultHandleGameClickEvent: ohne Nachfrage an den Server.
            ClickAction::Custom { id, payload } => {
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::custom_click_action_payload(&id, payload.as_ref()));
                }
            }
        }
    }

    // Chat-Eingabe oeffnen (T-Taste, s. window_event) -- nur im World-Modus und nur, wenn
    // gerade verbunden ist (unverbunden gibt's niemanden, an den man senden koennte).
    // Tab-Vervollstaendigung beim Server anfragen. Nur fuer Befehle, und dort nur fuer die
    // Texte, fuer die auch Vanilla anfragt -- welche das sind, entscheidet
    // ChatInput::wants_suggestions (sonst flaggt GrimAC mit ChatA/CrashH).
    //
    // Der Text geht MIT Schraegstrich raus, weil die Antwort ihre Ersetzungsposition als
    // Offset in genau diesen String angibt (s. packets::command_suggestion_payload).
    pub(crate) fn request_command_suggestions(&mut self) {
        let Some(feld) = self.chat_input.as_mut() else { return };
        if !feld.wants_suggestions() {
            feld.clear_suggestions();
            return;
        }
        // Wie Vanilla nur der Text bis zum Cursor.
        let text = feld.text_before_cursor().to_string();
        let id = feld.begin_request();
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::command_suggestion_payload(id, &text));
        }
    }

    pub(crate) fn handle_command_suggestions(&mut self, s: protocol::packets::CommandSuggestions) {
        if let Some(feld) = self.chat_input.as_mut() {
            feld.apply_suggestions(s.id, s.start, s.length, s.matches);
        }
    }

    pub(crate) fn open_chat(&mut self) {
        if self.mode != AppMode::World || self.conn_status != ConnStatus::Connected {
            return;
        }
        // Ohne Oeffnen-Klang: der Chat ist kein Fenster (Wunsch des Nutzers, wie beim Inventar).
        *self.chat_slide.borrow_mut() = None;
        *self.chat_gap.borrow_mut() = None;
        let mut feld = chat_input::ChatInput::new(String::new());
        feld.restore_history(self.chat_history.clone());
        self.chat_input = Some(feld);
        // Der offene Chat gibt die Maus frei (Links anklicken, s. chat_click): Zeiger mittig
        // unter die Eingabe (sie sitzt in der Mitte, s. chat_input_rect) -- nicht auf sie,
        // sonst laege er ueber dem Text. Ansicht wieder ganz unten.
        self.chat_scroll = 0;
        self.chat_history_scroll = 0;
        self.chat_scroll_glide.jump(0.0);
        self.chat_history_glide.jump(0.0);
        if let Some(gpu) = self.gpu.as_ref() {
            let eingabe = self.chat_input_rect(gpu);
            self.cursor_pos = (gpu.size().0 / 2.0, eingabe.y + eingabe.h + 2.0 * Self::chat_input_line_h(gpu));
        }
        // ALLE Tasten loslassen, wie Vanilla beim Oeffnen eines Screens (KeyBinding.
        // unpressAll). Das ist hier keine Kosmetik, sondern behebt zwei Fehler mit
        // derselben Ursache: solange die Chat-Eingabe offen ist, werden Tasten-Ereignisse
        // gar nicht mehr auf Aktionen angewandt (s. about_to_wait und window_event, beide
        // pruefen chat_input.is_none()) -- also kommt auch das LOSLASSEN nicht mehr an.
        //
        //   * Wer beim Oeffnen W haelt, laeuft danach fuer immer weiter: move_forward bleibt
        //     gesetzt, das Loslassen wurde verworfen.
        //   * Und die Chat-Taste selbst bleibt in keybind_physical_down als "unten" stehen.
        //     Beim naechsten Druck ist damit was_down noch true, apply_key_action sieht keine
        //     steigende Flanke und loest NICHT aus -- man muss zweimal druecken.
        //
        // clear_controls() raeumt genau beides (Halte-Aktionen und keybind_physical_down);
        // sein Kommentar beschreibt dieselbe Falle schon fuer den Fokusverlust, sie war nur
        // beim Chat nie angewandt.
        self.clear_controls();
    }

    // Enter waehrend offener Chat-Eingabe: leere Nachricht schliesst nur (kein Leer-Senden),
    // sonst senden + schliessen.
    // Chat schliessen und den Nachrichtenverlauf herausretten. Ueber DIESE Funktion, nicht
    // per `chat_input = None`: der Verlauf lebt im Eingabefeld, und wer das Feld einfach
    // wegwirft, wirft ihn mit weg. Genau das ist beim Escape-Weg passiert -- nach dem
    // Schliessen mit Escape holte Pfeil-oben nichts mehr herauf.
    pub(crate) fn close_chat(&mut self) {
        if let Some(feld) = self.chat_input.take() {
            if feld.history() != self.chat_history.as_slice() {
                self.chat_history = feld.history().to_vec();
                if let Err(e) = save_chat_history(&self.chat_history) {
                    self.push_log(format!("Chat-Verlauf nicht gespeichert: {e}"));
                }
            }
        }
        self.chat_history_hits.borrow_mut().clear();
        self.swallow_text_key = None;
        // Waehrend des Tippens gingen Tasten ans Feld, nicht an die Steuerung -- was jetzt
        // noch gehalten wird, gilt wieder (s. restore_held_controls).
        if self.mode == AppMode::World {
            self.restore_held_controls();
        }
    }

    pub(crate) fn send_chat_message(&mut self) {
        let Some(feld) = self.chat_input.as_mut() else {
            return;
        };
        let text = feld.take_for_send();
        self.close_chat();
        if text.is_empty() {
            return;
        }
        // Lokaler Diagnosebefehl, geht NICHT an den Server: schreibt den Stack der Haupthand
        // mit allen gelesenen Feldern und den rohen Component-Bytes in eine Datei. Gebaut fuer
        // Server-Items, die falsch aussehen (Golden Head auf mcpvp.club).
        if text == ".item" {
            self.dump_main_hand_item();
            return;
        }
        self.send_chat_text(text);
    }

    // Eine Zeile an den Server: Befehl oder (signierte) Chatnachricht -- aus dem Chatfeld oder
    // von einem Addon.
    pub(crate) fn send_chat_text(&mut self, text: String) {
        // Eigene Befehle der Addons gehen nicht an den Server; alles andere duerfen Addons
        // verwerfen oder umschreiben.
        if self.addon_command(&text) {
            return;
        }
        let text = match self.addon_hook(coral_addon::Event::ChatOut { text: text.clone() }) {
            coral_addon::Flow::Cancel => return,
            coral_addon::Flow::ReplaceText(t) => t,
            _ => text,
        };
        if self.outgoing_tx.is_none() {
            return;
        }
        if chat_input::sends_message(&text) {
            self.play_ui_sound(ui_sound::UiSound::Send);
        }
        let Some(tx) = &self.outgoing_tx else { return };
        // Befehle (mit '/') muessen als Chat-Command-Paket raus, nicht als Chat-Nachricht --
        // sonst flaggt Grim (ChatB). Fuehrenden Schraegstrich abschneiden.
        if let Some(command) = text.strip_prefix('/') {
            let _ = tx.send(protocol::packets::chat_command_payload(command));
            return;
        }
        let now_millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
        // Salt darf beliebig sein, muss aber in Signatur UND Paket derselbe Wert sein.
        let salt = now_millis.rotate_left(17);
        let signature = match &mut self.chat_signer {
            Some(signer) => match signer.sign(&text, now_millis, salt) {
                Ok(signature) => Some(signature),
                Err(e) => {
                    self.push_log(format!("Chat konnte nicht signiert werden: {e}"));
                    None
                }
            },
            None => None,
        };
        // tx erneut holen: das Signieren oben braucht &mut self.
        let Some(tx) = &self.outgoing_tx else { return };
        let _ = tx.send(protocol::packets::signed_chat_message_payload(&text, now_millis, salt, signature.as_ref()));
    }

    pub(crate) fn dump_main_hand_item(&mut self) {
        let slot = inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8);
        let meldung = match self.inventory.player.get(slot).and_then(|s| s.as_ref()) {
            None => "Haupthand leer".to_string(),
            Some(stack) => {
                let name = world::item_registry::item_name(stack.item_id).unwrap_or("?");
                let hex: String = stack.added_component_bytes.iter().map(|b| format!("{b:02x}")).collect();
                let text = format!("item: {name} (id {})\n\n{stack:#?}\n\nadded_component_bytes ({} Bytes):\n{hex}\n", stack.item_id, stack.added_component_bytes.len());
                let pfad = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from).unwrap_or_default().join("mc-item-dump.txt");
                match std::fs::write(&pfad, text) {
                    Ok(()) => format!("Item {name} nach {} geschrieben", pfad.display()),
                    Err(e) => format!("Item-Dump fehlgeschlagen: {e}"),
                }
            }
        };
        self.push_chat_message(vec![protocol::nbt::TextSpan { text: meldung, color: Some(0xAAAAAA), link: None, action: None, style: 0, shadow: None }]);
    }

    // Profilschluessel im Hintergrund holen (Netzaufruf zu Mojang). Ohne Anmeldung gibt es
    // keinen -- dann bleibt Chat unsigniert, was auf Servern mit
    // enforce-secure-profile=false voellig ausreicht.
    pub(crate) fn request_profile_key(&mut self) {
        let Some(credentials) = self.credentials.clone() else {
            return;
        };
        let tx = self.auth_tx.clone();
        self.tokio_rt.spawn(async move {
            match auth::fetch_profile_key(&credentials.access_token).await {
                Ok(key) => {
                    let _ = tx.send(AuthEvent::ProfileKey(key));
                }
                Err(e) => {
                    let _ = tx.send(AuthEvent::Status(format!("Kein Profilschlüssel: {e} -- Chat bleibt unsigniert")));
                }
            }
        });
    }

    // Profilschluessel ist da: Signierer bauen und dem Server die Sitzung melden. Das
    // Player-Session-Paket MUSS vor der ersten signierten Nachricht raus, sonst kennt der
    // Server den Schluessel nicht, gegen den er pruefen soll.
    pub(crate) fn start_chat_session(&mut self, key: auth::ProfileKey) {
        let Some(credentials) = &self.credentials else { return };
        let Some(sender) = uuid_bytes_from_hex(&credentials.profile_id) else {
            self.push_log("Konto-UUID unlesbar -- Chat bleibt unsigniert");
            return;
        };
        // Mojang signiert den Schluessel zusammen mit der Konto-UUID. Ein Offline-Mode-Server
        // fuehrt uns unter einer aus dem Namen abgeleiteten UUID -- die Signatur passt dort nie,
        // und der Server trennt beim Player-Session-Paket ("Invalid signature for profile
        // public key"), auch mit enforce-secure-profile=false. Dann lieber unsigniert.
        if self.own_uuid.is_some_and(|uuid| uuid != sender) {
            self.push_log("Server fuehrt eine andere UUID als das Konto (Offline-Mode) -- Chat bleibt unsigniert");
            return;
        }
        let session_id = net::random_session_id();
        match net::ChatSigner::new(&key.private_key_der, sender, session_id) {
            Ok(signer) => {
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::chat_session_update_payload(
                        session_id,
                        key.expires_at_millis,
                        &key.public_key_der,
                        &key.key_signature,
                    ));
                }
                self.chat_signer = Some(signer);
                self.push_log("Chat-Sitzung angemeldet -- Nachrichten werden signiert.");
            }
            Err(e) => self.push_log(format!("Profilschlüssel unbrauchbar: {e}")),
        }
    }
}
