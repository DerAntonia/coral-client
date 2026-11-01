// Text messen und in Vertices setzen, Sprites.

use super::*;

impl GpuState {
    // Pixelbreite eines Textes bei gegebener Skalierung -- fuer Caret-/Layout-Berechnung.
    // Mit dem Platz, den eine wechselnde Zahl darin reserviert hat (s. stable_numbers) --
    // sonst lagen Ausrichtung (mittig, rechtsbuendig) und Zeichnen auseinander.
    //
    // Gemessen wird der Text in der Sprache der Oberflaeche (s. i18n) -- dieselbe, in der
    // shape_text ihn zeichnet.
    pub(crate) fn measure_text(&self, text: &str, scale: f32) -> f32 {
        self.measure_raw(&i18n::tr(text), scale)
    }

    // Ohne Uebersetzung: fuer Text, der schon uebersetzt ist (Umbruch, Kuerzen).
    pub(crate) fn measure_raw(&self, text: &str, scale: f32) -> f32 {
        let _klein = self.small_label_font(scale);
        let text = &*self.layout_atlas().display_text(text);
        if self.plain_numbers.get() {
            return text.chars().map(|c| self.advance(c)).sum::<f32>() * scale;
        }
        self.stable_numbers.borrow().width(text, |c| self.advance(c)) * scale
    }

    // Vorschub eines Zeichens in Schriftpixeln; 0 fuer Zeichen, die die Schrift nicht hat
    // (die werden beim Zeichnen uebersprungen).
    pub(crate) fn advance(&self, ch: char) -> f32 {
        self.layout_atlas().glyph(ch).map_or(0.0, |g| g.advance_px)
    }

    // Startposition jedes Zeichens in Bildschirmpixeln relativ zum Zeilenanfang, mit
    // reserviertem Platz fuer wechselnde Zahlen. Fuer Zeilen, die aus mehreren UiTexts
    // bestehen (Farbabschnitte, s. App::build_server_hud) und trotzdem als EINE Zeile
    // ausgerichtet werden muessen.
    pub(crate) fn char_positions(&self, text: &str, scale: f32) -> (Vec<f32>, f32) {
        let _klein = self.small_label_font(scale);
        let text = &*self.layout_atlas().display_text(text);
        if self.plain_numbers.get() {
            let mut x = 0.0;
            let pos = text
                .chars()
                .map(|c| {
                    let hier = x;
                    x += self.advance(c) * scale;
                    hier
                })
                .collect();
            return (pos, x);
        }
        let (pos, w) = self.stable_numbers.borrow().layout(text, |c| self.advance(c));
        (pos.into_iter().map(|p| p * scale).collect(), w * scale)
    }

    // Zeilen, die nicht als EIN UiText gezeichnet werden (s. char_positions), selbst
    // anmelden -- shape_text sieht von ihnen nur die Stuecke.
    pub(crate) fn observe_numbers<'a>(&self, texts: impl IntoIterator<Item = &'a str>) {
        self.stable_numbers.borrow_mut().observe(texts, Instant::now(), |c| self.advance(c));
    }

    // Zeilenhoehe bei gegebenem Faktor -- damit sich Layout-Werte aus dem Font ergeben,
    // statt irgendwo als Zahl zu stehen, die zur Font passen muss.
    // Breite in Vanilla-Einheiten (Minecraft-Schrift, Fett ein Pixel mehr je Zeichen), egal in
    // welcher Schrift gezeichnet wird.
    pub(crate) fn vanilla_width(&self, text: &str, style: u8) -> f32 {
        let atlas = self.layout_atlas();
        let fett = if style & protocol::nbt::STYLE_BOLD != 0 { 1.0 } else { 0.0 };
        text.chars().map(|c| atlas.vanilla_advance(c) + fett).sum()
    }

    pub(crate) fn text_height(&self, scale: f32) -> f32 {
        self.layout_atlas().cell_height_px() * scale
    }

    // Schreibcursor fuer Text, der bei `text_y` beginnt: so hoch wie eine Zeile, aber an der
    // Oberkante der Grossbuchstaben ausgerichtet (s. FontAtlas::cap_top_px).
    pub(crate) fn caret_rect(&self, x: f32, text_y: f32, scale: f32) -> Rect {
        let y = (text_y + self.layout_atlas().cap_top_px() * scale).round();
        Rect { x, y, w: CARET_WIDTH_PX, h: self.text_height(scale) }
    }

    // Text mittig in ein Rechteck. Die Position wird auf ganze Pixel gerundet: ein
    // halber Pixel Versatz zerlegt den Bitmap-Font genauso wie eine Bruchskalierung,
    // nur subtiler -- die Glyphen wirken dann unruhig statt scharf.
    pub(crate) fn centered_text(&self, rect: Rect, text: String, color: [f32; 4], scale: f32) -> UiText {
        let width = self.measure_text(&text, scale);
        let height = self.text_height(scale);
        UiText { text, x: (rect.x + (rect.w - width) / 2.0).round(), y: (rect.y + (rect.h - height) / 2.0).round(), color, scale }
    }

    // Text zu Vertices. Nebenbei merkt sich jede Zeile, wo ihre Zeichen stehen (text_runs) --
    // daraus waehlt die Textauswahl aus (s. App::text_selection).
    //
    // Rand um den Text je nach TextShadow: Kontur (duenne schwarze Linie, genau EIN
    // Bildschirmpixel in alle Richtungen -- ein Schriftpixel liess bei Skala >= 2 Luecken an den
    // Diagonalen) oder Vanillas Schatten (ein Schriftpixel nach rechts unten, ein Viertel der
    // Farbe). Beides ist je Glyph EIN zusaetzliches Viereck, dessen Deckung der Shader rechnet
    // (s. font.wgsl) -- so blendet Text mit Rand sauber aus, statt grau zu werden.
    //
    // Welcher Rand gilt, stellt text_shadow_style ein; Marker in der Textliste (s.
    // push_shadow_marker) aendern ihn fuer die folgenden Texte (Chat, Hotbar, ...). Ebenso
    // die Schrift (push_font_marker).
    //
    // Rueckgabe: ab welchem Vertex (Index in `out`) welche Schrift gilt -- je Schrift ein
    // eigener Zeichenaufruf mit ihrem Atlas (s. draw_ui_top). Davor gilt die allgemeine.
    pub(crate) fn shape_text(&self, texts: &[UiText], screen_px: (f32, f32), out: &mut Vec<TextVertex>) -> Vec<(u32, FontKind)> {
        // Oberflaeche in der eingestellten Sprache (s. i18n): erst hier, nicht an den vielen
        // Stellen, die Beschriftungen bauen. measure_text uebersetzt genauso, Ausrichtung und
        // Zeichnen sehen also denselben Text.
        // Getippter Text (s. push_typed_marker) bleibt, wie er ist: wer "An" tippt, meint
        // nicht den Schalter "On".
        let uebersetzt: Vec<UiText>;
        let texts = if i18n::ui_language() == i18n::Language::German {
            texts
        } else {
            let mut getippt = false;
            uebersetzt = texts
                .iter()
                .map(|t| {
                    let text = if getippt { t.text.clone() } else { i18n::tr(&t.text).into_owned() };
                    getippt = is_typed_marker(t);
                    UiText { text, x: t.x, y: t.y, color: t.color, scale: t.scale }
                })
                .collect();
            &uebersetzt
        };
        let mut wechsel = Vec::new();
        self.text_clips.borrow_mut().clear();
        self.set_layout_font(None);
        self.shape_text_in(texts, screen_px, out, &mut wechsel);
        self.set_layout_font(None);
        wechsel
    }

    pub(crate) fn shape_text_in(&self, texts: &[UiText], screen_px: (f32, f32), out: &mut Vec<TextVertex>, wechsel: &mut Vec<(u32, FontKind)>) {
        let mut runs = self.text_runs.borrow_mut();
        runs.clear();
        let mut group = 0u32;
        let allgemein = (self.text_shadow_style, self.text_shadow_strength, self.text_shadow_half);
        let mut stil = allgemein;
        let mut aktuell = self.font_atlas.kind();
        // Schattenfarbe des Servers und Fettdruck fuer den naechsten Text (s. push_span_shadow,
        // push_bold_marker).
        let mut vom_server: Option<[f32; 4]> = None;
        let mut fett_naechster = false;
        let mut getippt_naechster = false;
        let mut fade = None;
        for t in texts {
            if let Some(an) = clip_marker_text(t) {
                self.text_clips.borrow_mut().push((out.len() as u32, an));
                continue;
            }
            if let Some(f) = fade_marker_text(t) {
                fade = f;
                continue;
            }
            if let Some(neu) = shadow_marker(t) {
                stil = neu.unwrap_or(allgemein);
                continue;
            }
            if let Some(farbe) = span_shadow_marker(t) {
                vom_server = Some(farbe);
                continue;
            }
            if is_bold_marker(t) {
                fett_naechster = true;
                continue;
            }
            if is_typed_marker(t) {
                getippt_naechster = true;
                continue;
            }
            if let Some(an) = plain_numbers_marker(t) {
                self.plain_numbers.set(an);
                continue;
            }
            if let Some(neu) = font_marker(t) {
                self.set_layout_font(neu);
                continue;
            }
            let _klein = self.small_label_font(t.scale);
            let atlas = self.layout_atlas();
            if atlas.kind() != aktuell {
                aktuell = atlas.kind();
                wechsel.push((out.len() as u32, aktuell));
            }
            if is_selection_boundary(t) {
                group += 1;
                continue;
            }
            let text_start = out.len();
            // t.y ist die Oberkante der Zeile; ueber den ascent zur Grundlinie.
            let scale = t.scale;
            let baseline_y = t.y + atlas.ascent_px() * scale;
            let mut glyphs = Vec::with_capacity(t.text.len());
            let (stil_hier, staerke, halb) = stil;
            let mut rand = if t.color[3] > 0.0 && staerke > 0.0 { stil_hier } else { TextShadow::Off };
            let mut randfarbe = match rand {
                TextShadow::Vanilla => vanilla_shadow_color(t.color),
                _ => [0.0, 0.0, 0.0, t.color[3]],
            };
            // Hat der Server die Schattenfarbe gesetzt, gilt sie wie in Vanilla (Alpha mal
            // Textdeckkraft, Alpha 0 = kein Schatten). Der Server waehlt nur die FARBE: die Art
            // (Kontur, Vanilla, Unten, Aussen) bleibt die eingestellte, und ist der Rand hier aus,
            // bleibt er aus.
            if let Some([r, g, b, a]) = vom_server.take().filter(|_| rand != TextShadow::Off) {
                if a > 0.0 {
                    randfarbe = [r, g, b, a * t.color[3]];
                } else {
                    rand = TextShadow::Off;
                }
            }
            if is_dark(t.color) && is_dark(randfarbe) {
                rand = TextShadow::Off;
            }
            // Erst alle Raender, dann alle Glyphen: sonst laege der Rand des naechsten Zeichens
            // ueber dem vorigen.
            let durchgaenge: &[bool] = if rand == TextShadow::Off { &[false] } else { &[true, false] };
            // Zeichenpositionen mit reserviertem Platz fuer wechselnde Zahlen (s.
            // stable_numbers) -- dieselben, mit denen measure_text ausgerichtet hat. Einmal je
            // Text, nicht je Durchgang.
            let (positionen, _) = self.char_positions(&t.text, scale);
            // Fett: je Zeichen ein Font-Pixel mehr Vorschub. Gezeichnet in echtem Fettdruck
            // (s. FontAtlas::place_bold), mittig in diesem Vorschub; wo es den nicht gibt
            // (Minecraft-Schrift, Symbole) wie Vanilla zweimal, bei 0 und +1 Pixel.
            let fett = std::mem::take(&mut fett_naechster);
            let mehr = if fett { scale } else { 0.0 };
            // Die schaerfste Lage auf dem Pixelraster (s. FontAtlas::sharpest_shift). Fett
            // bleibt, wo es ist: dort sitzen die Zeichen mittig im verbreiterten Vorschub.
            // Getippter Text auch: die Verschiebung haengt am ganzen Text, mit jedem Zeichen
            // ruckte sonst alles samt dem ersten Buchstaben, und der Cursor stand daneben.
            let getippt = std::mem::take(&mut getippt_naechster);
            let x = if fett || getippt { t.x } else { t.x + atlas.sharpest_shift(&atlas.display_text(&t.text), &positionen, t.x, scale) };
            for &ist_rand in durchgaenge {
                for (k, (ch, &rel)) in atlas.display_text(&t.text).chars().zip(&positionen).enumerate() {
                    let pen_x = x + rel + k as f32 * mehr;
                    if out.len() + VERTS_PER_QUAD * 2 > MAX_UI_GLYPHS * VERTS_PER_QUAD {
                        runs.push(TextRun { top: t.y, bottom: t.y + self.text_height(scale), glyphs, group });
                        return; // Buffer-Limit erreicht -- Rest weglassen statt ueberlaufen
                    }
                    let Some(g) = atlas.glyph(ch) else { continue };
                    let advance = g.advance_px * scale;
                    let echt_fett = if fett { atlas.place_bold(ch, scale) } else { None };
                    let (gesetzt, versaetze): (_, &[f32]) = match echt_fett {
                        Some((p, fett_vorschub)) => (Some((p, pen_x + (advance + mehr - fett_vorschub * scale) / 2.0)), &[0.0]),
                        None => (atlas.place(ch, scale).map(|p| (p, pen_x)), if fett { &[0.0, scale] } else { &[0.0] }),
                    };
                    if let Some((p, pen_x)) = gesetzt.filter(|(p, _)| p.size[0] > 0.0 && p.size[1] > 0.0) {
                        // Genau fuer diese Groesse gerasterte Glyphen (DM Sans) auf ganze
                        // Pixel setzen -- der Vorschub bleibt dabei unrund, der Startpunkt
                        // rastet ein, mit der Subpixel-Fassung fuer den Rest (s. snap_x). So
                        // trifft jedes Texel genau einen Bildschirmpixel, ohne dass sich
                        // Fehler aufsummieren.
                        let (p, pen_x) = atlas.snap_x(ch, scale, echt_fett.is_some(), p, pen_x);
                        let (ox, oy) = if p.snap { (pen_x, baseline_y.round()) } else { (pen_x, baseline_y) };
                        for &dx in versaetze {
                            let x0 = ox + dx + p.offset[0];
                            let y0 = oy + p.offset[1];
                            let (x1, y1) = (x0 + p.size[0], y0 + p.size[1]);
                            let erstes = out.len();
                            if ist_rand {
                                out.extend_from_slice(&text_border_quad(rand, staerke, [x0, y0, x1, y1], p.uv_min, p.uv_max, scale, p.snap, halb, randfarbe, screen_px));
                            } else {
                                out.extend_from_slice(&text_quad(x0, y0, x1, y1, p.uv_min, p.uv_max, t.color, screen_px));
                            }
                            if p.pack {
                                mark_pack_glyph(&mut out[erstes..]);
                            }
                        }
                    }
                    if !ist_rand {
                        glyphs.push((ch, pen_x, pen_x + advance + mehr));
                    }
                }
            }
            runs.push(TextRun { top: t.y, bottom: t.y + self.text_height(scale), glyphs, group });
            if let Some(f) = fade {
                fade_text_verts(&mut out[text_start..], f, screen_px.1);
            }
        }
        // ERST NACH dem Zeichnen lernen: so gilt fuer dieses Bild noch derselbe Stand, mit
        // dem measure_text vorhin ausgerichtet hat.
        drop(runs);
        self.set_layout_font(None);
        self.plain_numbers.set(false);
        let getippt = |i: usize| i > 0 && is_typed_marker(&texts[i - 1]);
        let mut ruhig = false;
        let ruhig_je_text: Vec<bool> = texts
            .iter()
            .map(|t| {
                if let Some(an) = plain_numbers_marker(t) {
                    ruhig = an;
                }
                ruhig
            })
            .collect();
        self.observe_numbers(
            texts.iter().enumerate().filter(|&(i, t)| !is_selection_boundary(t) && !getippt(i) && !ruhig_je_text[i]).map(|(_, t)| t.text.as_str()),
        );
    }

    // Eine Textzeile auf einer beliebig im Raum liegenden Flaeche (Text-Display). `project`
    // bildet einen Punkt der Textebene (Schriftpixel, x nach rechts, y nach unten) auf
    // Bildschirmpixel ab, oder None hinter der Kamera. `raster` = Bildschirmpixel je
    // Schriftpixel, waehlt nur die Rasterstufe der Glyphen (DM Sans). `rand`: Farbe der
    // Kontur, None = keine (s. world_text_border). `depth`: NDC-Tiefe je Punkt der Textebene
    // fuer den Welt-Durchgang (Schilder, s. font.wgsl::vs_world), steht in mode.w. Gesetzt
    // wird in der Schrift, mit der gerade gemessen wird (layout_atlas). Rueckgabe: Vorschub.
    //
    // Die Glyphen-Vierecke werden erst hier projiziert und dann ganz normal als Dreiecke
    // gezeichnet -- ohne perspektivisch korrekte Texturierung. Bei Glyphengroesse ist der
    // Unterschied unsichtbar.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shape_projected_text(
        &self,
        text: &str,
        x: f32,
        top: f32,
        color: [f32; 4],
        rand: Option<[f32; 4]>,
        raster: f32,
        project: &dyn Fn(f32, f32) -> Option<(f32, f32)>,
        depth: Option<&dyn Fn(f32, f32) -> f32>,
        screen_px: (f32, f32),
        out: &mut Vec<TextVertex>,
    ) -> f32 {
        const RICHTUNGEN: [(f32, f32); 8] = [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)];
        let atlas = self.layout_atlas();
        let tiefe = |x: f32, y: f32| depth.map_or(0.0, |f| f(x, y));
        let baseline = top + atlas.ascent_px();
        let mut pen = x;
        let (w, h) = screen_px;
        let ndc = |p: (f32, f32)| [(p.0 / w) * 2.0 - 1.0, 1.0 - (p.1 / h) * 2.0];
        for ch in atlas.display_text(text).chars() {
            let Some(g) = atlas.glyph(ch) else { continue };
            if let Some(p) = atlas.place(ch, raster).filter(|p| p.size[0] > 0.0 && p.size[1] > 0.0) {
                let x0 = pen + p.offset[0] / raster;
                let y0 = baseline + p.offset[1] / raster;
                let (x1, y1) = (x0 + p.size[0] / raster, y0 + p.size[1] / raster);
                if let (Some(a), Some(b), Some(c), Some(d)) = (project(x0, y0), project(x1, y0), project(x1, y1), project(x0, y1)) {
                    let (u0, v0, u1, v1) = (p.uv_min[0], p.uv_min[1], p.uv_max[0], p.uv_max[1]);
                    let (za, zb, zc, zd) = (tiefe(x0, y0), tiefe(x1, y0), tiefe(x1, y1), tiefe(x0, y1));
                    let mut quad = |dx: f32, dy: f32, color: [f32; 4]| {
                        let v = |q: (f32, f32), u: f32, vv: f32, z: f32| TextVertex {
                            position: ndc((q.0 + dx, q.1 + dy)),
                            uv: [u, vv],
                            color,
                            bounds: [0.0; 4],
                            mode: [0.0, 0.0, 0.0, z],
                        };
                        let (va, vb, vc, vd) = (v(a, u0, v0, za), v(b, u1, v0, zb), v(c, u1, v1, zc), v(d, u0, v1, zd));
                        let erstes = out.len();
                        out.extend_from_slice(&[va, vb, vc, va, vc, vd]);
                        if p.pack {
                            mark_pack_glyph(&mut out[erstes..]);
                        }
                    };
                    // Dieselbe Kontur wie auf dem Bildschirm (s. shape_text).
                    if let Some(rand) = rand {
                        for (dx, dy) in RICHTUNGEN {
                            quad(dx, dy, rand);
                        }
                    }
                    quad(0.0, 0.0, color);
                }
            }
            pen += g.advance_px;
        }
        pen - x
    }

    // Die Kontur eines Abschnitts Weltschrift (Schild, Text-Display): schwarz, oder in der
    // Schattenfarbe, die der Server dem Abschnitt gibt (Alpha 0 = keine). `erlaubt` = der Ort
    // zeichnet ueberhaupt einen Rand -- beim Text-Display entscheidet das der Server ueber
    // sein Schatten-Flag, wie in Vanilla.
    pub(crate) fn world_text_border(&self, color: [f32; 4], shadow: Option<u32>, erlaubt: bool) -> Option<[f32; 4]> {
        if !erlaubt || self.text_shadow_style == TextShadow::Off || color[3] <= 0.0 {
            return None;
        }
        let rand = match shadow.and_then(ego_name::shadow) {
            None => [0.0, 0.0, 0.0, color[3]],
            Some(argb) => {
                let [r, g, b, a] = argb_to_rgba(argb as i32);
                [r, g, b, a * color[3]]
            }
        };
        (rand[3] > 0.0 && !(is_dark(color) && is_dark(rand))).then_some(rand)
    }

    // UiSprites zu Vertices. Rueckgabe: die zusammenhaengenden Laeufe (Atlas, Startvertex,
    // Anzahl) -- ein Draw-Call je Lauf, weil ein Atlas-Wechsel eine andere Bind-Group
    // braucht. Die Reihenfolge der Sprites bleibt erhalten (Hintergrund vor Icons).
    pub(crate) fn shape_sprites(sprites: &[UiSprite], screen_px: (f32, f32), out: &mut Vec<TextVertex>) -> Vec<(SpriteAtlas, u32, u32)> {
        let mut runs: Vec<(SpriteAtlas, u32, u32)> = Vec::new();
        for sprite in sprites.iter().take(MAX_UI_SPRITES) {
            let [u0, v0, u1, v1] = sprite.uv;
            let uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
            // Glint-Zwillinge tragen in der Farbe ihre Glint-UV: lokal 0..1 ueber das Quad, mal
            // Vanillas Item-Massstab (s. render::glint::GLINT_SCALE_ITEM); Alpha bleibt.
            let lokal = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            let s = render::glint::GLINT_SCALE_ITEM;
            let vertex = |i: usize| TextVertex {
                position: pixel_to_ndc(sprite.corners[i][0], sprite.corners[i][1], screen_px),
                uv: uvs[i],
                // z = Helligkeit des Icons darunter (Mittel der Toenung), s. sprite.wgsl::fs_glint.
                // Aus dem UI-Atlas (flache Items): x = -1 heisst "Glint-UV aus der Atlas-UV", s.
                // sprite.wgsl::fs_glint -- ein Icon aus vielen Pixel-Quads (Angel) bekam sonst
                // auf jedem Quad den ganzen Glint.
                color: if sprite.atlas == SpriteAtlas::GlintUi {
                    [-1.0, 0.0, (sprite.color[0] + sprite.color[1] + sprite.color[2]) / 3.0, sprite.color[3]]
                } else if sprite.atlas.is_glint() {
                    [lokal[i][0] * s, lokal[i][1] * s, (sprite.color[0] + sprite.color[1] + sprite.color[2]) / 3.0, sprite.color[3]]
                } else {
                    sprite.color
                },
                bounds: [0.0; 4],
                mode: [0.0; 4],
            };
            let start = out.len() as u32;
            out.extend_from_slice(&[vertex(0), vertex(1), vertex(2), vertex(0), vertex(2), vertex(3)]);
            match runs.last_mut() {
                Some((atlas, _, count)) if *atlas == sprite.atlas => *count += VERTS_PER_QUAD as u32,
                _ => runs.push((sprite.atlas, start, VERTS_PER_QUAD as u32)),
            }
        }
        runs
    }

    pub(crate) fn sprite_bind_group(&self, atlas: SpriteAtlas) -> &wgpu::BindGroup {
        match atlas {
            SpriteAtlas::ServerIcons => self.server_icons.as_ref().map_or(&self.ui_atlas_bind_group, |(_, b)| b),
            SpriteAtlas::PackIcons => self.pack_icons.as_ref().map_or(&self.ui_atlas_bind_group, |(_, b)| b),
            SpriteAtlas::Logos => self.logos.as_ref().map_or(&self.ui_atlas_bind_group, |(_, b)| b),
            SpriteAtlas::Ui | SpriteAtlas::GlintUi => &self.ui_atlas_bind_group,
            SpriteAtlas::Block | SpriteAtlas::GlintBlock => &self.block_atlas_sprite_bind_group,
            SpriteAtlas::Entity | SpriteAtlas::GlintEntity => &self.entity_atlas_sprite_bind_group,
        }
    }

    // Zeichnet die Sprite-Laeufe (s. shape_sprites) aus dem schon gebundenen
    // sprite_vertex_buffer. Glint-Laeufe wechseln auf die additive Glint-Pipeline und binden
    // zusaetzlich Glint-Textur + Matrix (Gruppe 1).
    pub(crate) fn draw_sprite_runs(&self, pass: &mut wgpu::RenderPass<'_>, runs: &[(SpriteAtlas, u32, u32)]) {
        let mut glint_aktiv = None;
        for (atlas, start, count) in runs {
            if glint_aktiv != Some(atlas.is_glint()) {
                glint_aktiv = Some(atlas.is_glint());
                if atlas.is_glint() {
                    pass.set_pipeline(&self.glint_sprite_pipeline);
                    pass.set_bind_group(1, &self.glint_gui_bind_group, &[]);
                } else {
                    pass.set_pipeline(&self.sprite_pipeline);
                }
            }
            pass.set_bind_group(0, self.sprite_bind_group(*atlas), &[]);
            pass.draw(*start..(*start + *count), 0..1);
        }
    }
}
