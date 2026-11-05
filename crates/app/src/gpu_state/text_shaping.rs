// Text messen und in Vertices setzen, Sprites.

use super::*;

// Fertig gesetzte Texte: dieselbe Zeile an derselben Stelle (HUD, Scoreboard, Tab-Liste,
// stehender Chat) ergibt jeden Frame dieselben Vertices. Der Schluessel enthaelt alles, wovon
// das Setzen abhaengt -- Atlas, Anzeigetext, Zeichenlagen (samt reserviertem Zahlenplatz),
// Lage, Farben, Rand, Fett --, ein Treffer kopiert nur noch die Vertices.
#[derive(Default)]
pub(crate) struct ShapedCache {
    eintraege: std::collections::HashMap<ShapedKey, ShapedEntry>,
    // Lagen-Cache (s. LayoutEntry), unter dem Hash seines Schluessels.
    lagen: rustc_hash::FxHashMap<u64, LayoutEntry>,
    // Vertices aller Eintraege zusammen (s. SHAPED_PRUNE_VERTS).
    verts_gesamt: usize,
    // Je shape_text-Aufruf eins weiter; was lange nicht gebraucht wurde, faellt raus.
    runde: u64,
}

// Ein Text in Schriftpixeln, unabhaengig von Lage und Faktor: je Glyphen-Viereck das Rechteck
// relativ zu Zeilenanfang und Grundlinie. Gilt, wo nichts einrastet (s.
// FontAtlas::linear_raster) -- dann wird beim Zeichnen nur mal Faktor genommen und
// verschoben. Weltbeschriftungen aendern Lage und Faktor stufenlos jedes Bild; mit dem fertigen
// Satz (ShapedKey) trafen sie beim Gehen nie und wurden jedes Bild ganz neu gesetzt.
struct LayoutEntry {
    generation: u64,
    atlas: FontKind,
    stufe: u8,
    fett: bool,
    text: Box<str>,
    // Zeichenlagen mit reserviertem Zahlenplatz (s. stable_numbers), nur bei Text mit Ziffern;
    // ohne haengen sie allein am Text.
    positionen: Box<[u32]>,
    quads: Vec<UnitQuad>,
    // Je Zeichen Anfang und Ende fuer die Textauswahl (s. TextRun), in Schriftpixeln.
    glyphs: Vec<(char, f32, f32)>,
    runde: u64,
    // Zuletzt daraus gesetzt (s. layout_stand): an derselben Stelle in denselben Farben (HUD in
    // der Minecraft-Schrift, stehende Kamera) nur kopieren.
    zuletzt: Option<[u32; 16]>,
    verts: Vec<TextVertex>,
}

#[derive(Clone, Copy)]
struct UnitQuad {
    rect: [f32; 4],
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    pack: bool,
}

// Was ausser dem Lagen-Eintrag in die gesetzten Vertices eingeht.
fn layout_stand(t: &UiText, screen_px: (f32, f32), rand: TextShadow, staerke: f32, halb: bool, randfarbe: [f32; 4]) -> [u32; 16] {
    let mut s = [0u32; 16];
    s[..5].copy_from_slice(&[t.x, t.y, t.scale, screen_px.0, screen_px.1].map(f32::to_bits));
    s[5..9].copy_from_slice(&t.color.map(f32::to_bits));
    s[9..13].copy_from_slice(&randfarbe.map(f32::to_bits));
    s[13] = staerke.to_bits();
    s[14] = rand as u32;
    s[15] = u32::from(halb);
    s
}

// Die Glyphen-Vierecke eines Textes in Schriftpixeln (s. LayoutEntry), gesetzt wie in
// shape_text_in bei Faktor `scale` und durch ihn geteilt. `lagen`: Zeichenanfaenge in
// Schriftpixeln. None, falls doch ein Zeichen einrastet -- dann gilt die Lage nur genau dort.
fn unit_layout(atlas: &FontAtlas, anzeige: &str, lagen: &[f32], fett: bool, scale: f32) -> Option<(Vec<UnitQuad>, Vec<(char, f32, f32)>)> {
    let mehr = if fett { 1.0 } else { 0.0 };
    let mut quads = Vec::with_capacity(anzeige.len());
    let mut glyphs = Vec::with_capacity(anzeige.len());
    for (k, (ch, &rel)) in anzeige.chars().zip(lagen).enumerate() {
        let stift = rel + k as f32 * mehr;
        let Some(g) = atlas.glyph(ch) else { continue };
        let vorschub = g.advance_px;
        let echt_fett = if fett { atlas.place_bold(ch, scale) } else { None };
        let (gesetzt, versaetze): (_, &[f32]) = match echt_fett {
            Some((p, fett_vorschub)) => (Some((p, stift + (vorschub + mehr - fett_vorschub) / 2.0)), &[0.0]),
            None => (atlas.place(ch, scale).map(|p| (p, stift)), if fett { &[0.0, 1.0] } else { &[0.0] }),
        };
        if let Some((p, x)) = gesetzt.filter(|(p, _)| p.size[0] > 0.0 && p.size[1] > 0.0) {
            if p.snap {
                return None;
            }
            for &dx in versaetze {
                let x0 = x + dx + p.offset[0] / scale;
                let y0 = p.offset[1] / scale;
                quads.push(UnitQuad { rect: [x0, y0, x0 + p.size[0] / scale, y0 + p.size[1] / scale], uv_min: p.uv_min, uv_max: p.uv_max, pack: p.pack });
            }
        }
        glyphs.push((ch, stift, stift + vorschub + mehr));
    }
    Some((quads, glyphs))
}

// Zoom und Ausblenden (s. zoom_marker_text, fade_marker_text) verschieben die gesetzten
// Vertices eines Textes danach noch.
fn zoom_fade(verts: &mut [TextVertex], zoom: Option<(f32, f32, f32)>, fade: Option<ScrollFade>, screen_px: (f32, f32)) {
    if let Some((zx, zy, s)) = zoom {
        let mitte = [zx / screen_px.0 * 2.0 - 1.0, 1.0 - zy / screen_px.1 * 2.0];
        for v in &mut *verts {
            v.position = std::array::from_fn(|i| mitte[i] + (v.position[i] - mitte[i]) * s);
        }
    }
    if let Some(f) = fade {
        fade_text_verts(verts, f, screen_px.1);
    }
}

#[derive(PartialEq, Eq, Hash)]
struct ShapedKey {
    generation: u64,
    atlas: FontKind,
    text: String,
    positionen: Vec<u32>,
    // x, y, Faktor, Bildschirm (Bits).
    lage: [u32; 5],
    farbe: [u32; 4],
    randfarbe: [u32; 4],
    // Randart, Staerke (Bits), halbe Verschiebung, fett, getippt.
    rand: (u8, u32, bool, bool, bool),
}

struct ShapedEntry {
    verts: Vec<TextVertex>,
    glyphs: Vec<(char, f32, f32)>,
    runde: u64,
}

// So viele Aufrufe darf ein Eintrag ungenutzt bleiben (zwei, drei je Frame).
const SHAPED_KEEP_ROUNDS: u64 = 240;
const SHAPED_PRUNE_AT: usize = 2048;
// Auch nach Menge aufraeumen: lange Zeilen mit Rand (Chat) haben tausende Vertices zu je 68
// Bytes, unter SHAPED_PRUNE_AT Eintraegen lagen so bis zu 76 MB laengst unbenutzter Saetze
// (heaptrack 2026-11-05). 65536 Vertices sind gut 4 MB.
const SHAPED_PRUNE_VERTS: usize = 65_536;

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
        self.shape_text_split(texts, screen_px, out, &[]).0
    }

    // Wie shape_text, viele Stuecke in EINEM Aufruf (die verdeckbaren Namensschilder, s.
    // prepare_occluded_labels): `ende_bei` sind Indizes in `texts`, an denen ein Stueck endet;
    // dazu je Stueck der Vertex-Index in `out`, an dem es endet.
    pub(crate) fn shape_text_split(&self, texts: &[UiText], screen_px: (f32, f32), out: &mut Vec<TextVertex>, ende_bei: &[usize]) -> (Vec<(u32, FontKind)>, Vec<u32>) {
        let gestartet = Instant::now();
        let mut enden = Vec::with_capacity(ende_bei.len());
        let wechsel = self.shape_text_timed(texts, screen_px, out, ende_bei, &mut enden);
        // Beim Puffer-Limit bricht das Formen ab: der Rest endet, wo es aufhoerte.
        enden.resize(ende_bei.len(), out.len() as u32);
        self.text_shape_ms.set(self.text_shape_ms.get() + gestartet.elapsed().as_secs_f32() * 1000.0);
        (wechsel, enden)
    }

    fn shape_text_timed(&self, texts: &[UiText], screen_px: (f32, f32), out: &mut Vec<TextVertex>, ende_bei: &[usize], enden: &mut Vec<u32>) -> Vec<(u32, FontKind)> {
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
        {
            let mut cache = self.shaped_cache.borrow_mut();
            cache.runde += 1;
            if cache.eintraege.len() > SHAPED_PRUNE_AT || cache.verts_gesamt > SHAPED_PRUNE_VERTS {
                let jetzt = cache.runde;
                cache.eintraege.retain(|_, e| jetzt - e.runde < SHAPED_KEEP_ROUNDS);
                cache.verts_gesamt = cache.eintraege.values().map(|e| e.verts.len()).sum();
            }
            if cache.lagen.len() > SHAPED_PRUNE_AT {
                let jetzt = cache.runde;
                cache.lagen.retain(|_, e| jetzt - e.runde < SHAPED_KEEP_ROUNDS);
            }
        }
        self.text_clips.borrow_mut().clear();
        self.set_layout_font(None);
        self.shape_text_in(texts, screen_px, out, &mut wechsel, ende_bei, enden);
        self.set_layout_font(None);
        wechsel
    }

    pub(crate) fn shape_text_in(
        &self,
        texts: &[UiText],
        screen_px: (f32, f32),
        out: &mut Vec<TextVertex>,
        wechsel: &mut Vec<(u32, FontKind)>,
        ende_bei: &[usize],
        enden: &mut Vec<u32>,
    ) {
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
        let mut zoom = None;
        let mut stufenlos = false;
        for (i, t) in texts.iter().enumerate() {
            while ende_bei.get(enden.len()).is_some_and(|&e| e <= i) {
                enden.push(out.len() as u32);
            }
            if let Some(z) = zoom_marker_text(t) {
                zoom = z;
                continue;
            }
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
            if let Some(an) = smooth_scale_marker(t) {
                stufenlos = an;
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
            // Wo nichts einrastet, ueber den Lagen-Cache (s. LayoutEntry). Kein sharpest_shift
            // dort: ohne eingerastete Zeichen verschiebt er nicht. Stufenlos skalierter Text
            // (s. push_smooth_scale_marker) rastet auch an den genauen Faktoren nicht ein.
            let raster = if stufenlos { atlas.smooth_raster(scale) } else { atlas.linear_raster(scale).map(|s| (s, scale)) };
            if let Some(stufe) = raster {
                if let Some(glyphs) = self.shape_linear(atlas, t, stufe, fett_naechster, (rand, staerke, halb, randfarbe), screen_px, out) {
                    fett_naechster = false;
                    getippt_naechster = false;
                    runs.push(TextRun { top: t.y, bottom: t.y + self.text_height(scale), glyphs, group });
                    zoom_fade(&mut out[text_start..], zoom, fade, screen_px);
                    continue;
                }
            }
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
            let anzeige = atlas.display_text(&t.text);
            // Schon einmal genau so gesetzt? (Zoom und Ausblenden verschieben die Vertices
            // danach noch, die laufen immer neu.)
            let schluessel = (zoom.is_none() && fade.is_none()).then(|| ShapedKey {
                generation: font_atlas::glyph_generation(),
                atlas: atlas.kind(),
                text: anzeige.to_string(),
                positionen: positionen.iter().map(|p| p.to_bits()).collect(),
                lage: [t.x.to_bits(), t.y.to_bits(), scale.to_bits(), screen_px.0.to_bits(), screen_px.1.to_bits()],
                farbe: t.color.map(f32::to_bits),
                randfarbe: randfarbe.map(f32::to_bits),
                rand: (rand as u8, staerke.to_bits(), halb, fett, getippt),
            });
            if let Some(k) = &schluessel {
                let mut cache = self.shaped_cache.borrow_mut();
                let runde = cache.runde;
                if let Some(e) = cache.eintraege.get_mut(k).filter(|e| out.len() + e.verts.len() <= MAX_UI_GLYPHS * VERTS_PER_QUAD) {
                    e.runde = runde;
                    out.extend_from_slice(&e.verts);
                    runs.push(TextRun { top: t.y, bottom: t.y + self.text_height(scale), glyphs: e.glyphs.clone(), group });
                    continue;
                }
            }
            let x = if fett || getippt { t.x } else { t.x + atlas.sharpest_shift(&anzeige, &positionen, t.x, scale) };
            for &ist_rand in durchgaenge {
                for (k, (ch, &rel)) in anzeige.chars().zip(&positionen).enumerate() {
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
            if let Some(k) = schluessel {
                let mut cache = self.shaped_cache.borrow_mut();
                let runde = cache.runde;
                let verts = out[text_start..].to_vec();
                cache.verts_gesamt += verts.len();
                if let Some(alt) = cache.eintraege.insert(k, ShapedEntry { verts, glyphs: glyphs.clone(), runde }) {
                    cache.verts_gesamt -= alt.verts.len();
                }
            }
            runs.push(TextRun { top: t.y, bottom: t.y + self.text_height(scale), glyphs, group });
            zoom_fade(&mut out[text_start..], zoom, fade, screen_px);
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

    // Einen Text bei einem Faktor ohne Einrasten (s. FontAtlas::linear_raster, `stufe`) ueber
    // den Lagen-Cache setzen, wie es der gewoehnliche Weg in shape_text_in taete. `lage_faktor`:
    // bei ihm wird ein fehlender Eintrag gesetzt (s. FontAtlas::smooth_raster). Rueckgabe: die
    // Zeichen fuer die Textauswahl, None = nicht gesetzt (Puffer knapp, Hash-Kollision) -- dann
    // setzt der gewoehnliche Weg.
    #[allow(clippy::too_many_arguments)]
    fn shape_linear(
        &self,
        atlas: &FontAtlas,
        t: &UiText,
        (stufe, lage_faktor): (u8, f32),
        fett: bool,
        (rand, staerke, halb, randfarbe): (TextShadow, f32, bool, [f32; 4]),
        screen_px: (f32, f32),
        out: &mut Vec<TextVertex>,
    ) -> Option<Vec<(char, f32, f32)>> {
        use std::hash::{Hash, Hasher};
        let scale = t.scale;
        let anzeige = atlas.display_text(&t.text);
        let vorschub = |c: char| atlas.glyph(c).map_or(0.0, |g| g.advance_px);
        let mit_ziffern = !self.plain_numbers.get() && anzeige.bytes().any(|b| b.is_ascii_digit());
        let positionen: Vec<u32> = if mit_ziffern {
            self.stable_numbers.borrow().layout(&anzeige, vorschub).0.into_iter().map(f32::to_bits).collect()
        } else {
            Vec::new()
        };
        let generation = font_atlas::glyph_generation();
        let art = atlas.kind();
        let schluessel = {
            let mut h = rustc_hash::FxHasher::default();
            (generation, art, stufe, fett).hash(&mut h);
            anzeige.hash(&mut h);
            positionen.hash(&mut h);
            h.finish()
        };
        let mut cache = self.shaped_cache.borrow_mut();
        let runde = cache.runde;
        match cache.lagen.get(&schluessel) {
            Some(e) if e.generation == generation && e.atlas == art && e.stufe == stufe && e.fett == fett && *e.text == *anzeige && *e.positionen == *positionen => {}
            // Ein anderer Text unter demselben Hash: nicht ueberschreiben.
            Some(_) => return None,
            None => {
                let lagen: Vec<f32> = if mit_ziffern {
                    positionen.iter().copied().map(f32::from_bits).collect()
                } else {
                    let mut x = 0.0;
                    anzeige
                        .chars()
                        .map(|c| {
                            let hier = x;
                            x += vorschub(c);
                            hier
                        })
                        .collect()
                };
                let (quads, glyphs) = unit_layout(atlas, &anzeige, &lagen, fett, lage_faktor)?;
                let neu = LayoutEntry {
                    generation,
                    atlas: art,
                    stufe,
                    fett,
                    text: anzeige.as_ref().into(),
                    positionen: positionen.into(),
                    quads,
                    glyphs,
                    runde,
                    zuletzt: None,
                    verts: Vec::new(),
                };
                cache.lagen.insert(schluessel, neu);
            }
        }
        let e = cache.lagen.get_mut(&schluessel)?;
        let durchgaenge = if rand == TextShadow::Off { 1 } else { 2 };
        // Mit demselben Rand wie shape_text_in je Zeichen: reicht der Platz nicht fuer alles,
        // kuerzt der gewoehnliche Weg wie bisher.
        if out.len() + (e.quads.len() * durchgaenge + 2) * VERTS_PER_QUAD > MAX_UI_GLYPHS * VERTS_PER_QUAD {
            return None;
        }
        e.runde = runde;
        let stand = layout_stand(t, screen_px, rand, staerke, halb, randfarbe);
        if e.zuletzt == Some(stand) {
            out.extend_from_slice(&e.verts);
        } else {
            let anfang = out.len();
            let grundlinie = t.y + atlas.ascent_px() * scale;
            let ecken = |q: &UnitQuad| [t.x + q.rect[0] * scale, grundlinie + q.rect[1] * scale, t.x + q.rect[2] * scale, grundlinie + q.rect[3] * scale];
            // Erst alle Raender, dann alle Glyphen (s. shape_text_in).
            if rand != TextShadow::Off {
                for q in &e.quads {
                    let erstes = out.len();
                    out.extend_from_slice(&text_border_quad(rand, staerke, ecken(q), q.uv_min, q.uv_max, scale, false, halb, randfarbe, screen_px));
                    if q.pack {
                        mark_pack_glyph(&mut out[erstes..]);
                    }
                }
            }
            for q in &e.quads {
                let [x0, y0, x1, y1] = ecken(q);
                let erstes = out.len();
                out.extend_from_slice(&text_quad(x0, y0, x1, y1, q.uv_min, q.uv_max, t.color, screen_px));
                if q.pack {
                    mark_pack_glyph(&mut out[erstes..]);
                }
            }
            e.verts.clear();
            e.verts.extend_from_slice(&out[anfang..]);
            e.zuletzt = Some(stand);
        }
        Some(e.glyphs.iter().map(|&(c, a, b)| (c, t.x + a * scale, t.x + b * scale)).collect())
    }

    // Eine Textzeile auf einer beliebig im Raum liegenden Flaeche (Text-Display). `project`
    // bildet einen Punkt der Textebene (Schriftpixel, x nach rechts, y nach unten) auf
    // Bildschirmpixel ab, oder None hinter der Kamera. `raster` = Bildschirmpixel je
    // Schriftpixel, waehlt nur die Rasterstufe der Glyphen (DM Sans). `rand`: Art, Staerke
    // und Farbe des Randes, None = keiner (s. world_text_border, projected_border). `depth`:
    // NDC-Tiefe je Punkt der Textebene fuer den Welt-Durchgang (Schilder, s.
    // font.wgsl::vs_world), steht in TextVertex::depth. Gesetzt wird in der Schrift, mit der
    // gerade gemessen wird (layout_atlas). Rueckgabe: Vorschub.
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
        rand: Option<(ShadowRun, [f32; 4])>,
        raster: f32,
        project: &dyn Fn(f32, f32) -> Option<(f32, f32)>,
        depth: Option<&dyn Fn(f32, f32) -> f32>,
        screen_px: (f32, f32),
        out: &mut Vec<TextVertex>,
    ) -> f32 {
        let atlas = self.layout_atlas();
        let tiefe = |x: f32, y: f32| depth.map_or(0.0, |f| f(x, y));
        let baseline = top + atlas.ascent_px();
        let mut pen = x;
        let (w, h) = screen_px;
        let ndc = |p: (f32, f32)| [(p.0 / w) * 2.0 - 1.0, 1.0 - (p.1 / h) * 2.0];
        // Ein Viereck der Textebene (x0, y0, x1, y1) projiziert, als zwei Dreiecke; None, wenn
        // eine Ecke hinter der Kamera liegt.
        let viereck = |[x0, y0, x1, y1]: [f32; 4], [u0, v0, u1, v1]: [f32; 4], color: [f32; 4], bounds: [f32; 4], mode: [f32; 4], pack: bool| {
            let mode = if pack { [mode[0] + crate::text_draw::TEXT_MODE_PACK, mode[1], mode[2], mode[3]] } else { mode };
            let v = |x: f32, y: f32, u: f32, vv: f32| project(x, y).map(|q| TextVertex { position: ndc(q), uv: [u, vv], color, bounds, mode, depth: tiefe(x, y) });
            let (va, vb, vc, vd) = (v(x0, y0, u0, v0)?, v(x1, y0, u1, v0)?, v(x1, y1, u1, v1)?, v(x0, y1, u0, v1)?);
            Some([va, vb, vc, va, vc, vd])
        };
        // Mit Rand erst alle Raender, dann alle Glyphen (wie shape_text): sonst laege der Rand
        // des naechsten Zeichens ueber dem vorigen.
        let mut glyphen: Vec<[TextVertex; 6]> = Vec::new();
        for ch in atlas.display_text(text).chars() {
            let Some(g) = atlas.glyph(ch) else { continue };
            if let Some(p) = atlas.place(ch, raster).filter(|p| p.size[0] > 0.0 && p.size[1] > 0.0) {
                let x0 = pen + p.offset[0] / raster;
                let y0 = baseline + p.offset[1] / raster;
                let (x1, y1) = (x0 + p.size[0] / raster, y0 + p.size[1] / raster);
                let uv = [p.uv_min[0], p.uv_min[1], p.uv_max[0], p.uv_max[1]];
                if let Some(((stil, staerke, halb), farbe)) = rand {
                    let kanten = (|| Some((project(x0, y0)?, project(x1, y0)?, project(x0, y1)?)))();
                    if let Some((ecken, rand_uv, mode, farbe)) = kanten.and_then(|k| projected_border(stil, staerke, halb, farbe, [x0, y0, x1, y1], uv, k)) {
                        if let Some(q) = viereck(ecken, rand_uv, farbe, uv, mode, p.pack) {
                            out.extend_from_slice(&q);
                        }
                    }
                }
                if let Some(q) = viereck([x0, y0, x1, y1], uv, color, [0.0; 4], [0.0; 4], p.pack) {
                    if rand.is_some() {
                        glyphen.push(q);
                    } else {
                        out.extend_from_slice(&q);
                    }
                }
            }
            pen += g.advance_px;
        }
        for q in &glyphen {
            out.extend_from_slice(q);
        }
        pen - x
    }

    // Der Rand eines Abschnitts Weltschrift (Schild, Text-Display): die Art samt Staerke aus
    // `lauf` (None = die allgemeine), Farbe wie beim Bildschirmtext (s. shape_text_in) --
    // Vanilla ein Viertel der Textfarbe, sonst schwarz, oder die Schattenfarbe, die der Server
    // dem Abschnitt gibt (Alpha 0 = keiner). `erlaubt` = der Ort zeichnet ueberhaupt einen
    // Rand -- beim Text-Display entscheidet das der Server ueber sein Schatten-Flag, wie in
    // Vanilla.
    pub(crate) fn world_text_border(&self, color: [f32; 4], shadow: Option<u32>, erlaubt: bool, lauf: Option<ShadowRun>) -> Option<(ShadowRun, [f32; 4])> {
        let lauf = lauf.unwrap_or((self.text_shadow_style, self.text_shadow_strength, self.text_shadow_half));
        if !erlaubt || lauf.0 == TextShadow::Off || lauf.1 <= 0.0 || color[3] <= 0.0 {
            return None;
        }
        let rand = match shadow.and_then(ego_name::shadow) {
            None if lauf.0 == TextShadow::Vanilla => vanilla_shadow_color(color),
            None => [0.0, 0.0, 0.0, color[3]],
            Some(argb) => {
                let [r, g, b, a] = argb_to_rgba(argb as i32);
                [r, g, b, a * color[3]]
            }
        };
        (rand[3] > 0.0 && !(is_dark(color) && is_dark(rand))).then_some((lauf, rand))
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
                depth: 0.0,
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

// Der Rand einer Schildglyphe fuer font.wgsl::vs_sign: Art, Wert (Dicke der Kontur bzw.
// Versatz des Vanilla-Schattens in Schriftpixeln) und Farbe samt Staerke -- dieselben Groessen
// wie projected_border, nur ohne das, was von der Projektion abhaengt.
pub(crate) fn sign_border_mode(stil: TextShadow, staerke: f32, halb: bool, farbe: [f32; 4]) -> Option<(f32, f32, [f32; 4])> {
    let mal = |k: f32| [farbe[0], farbe[1], farbe[2], farbe[3] * k];
    match stil {
        TextShadow::Off => None,
        TextShadow::Outline => Some((crate::text_draw::TEXT_MODE_OUTLINE_WORLD, staerke.clamp(0.5, 4.0), farbe)),
        TextShadow::Vanilla => Some((crate::text_draw::TEXT_MODE_SHADOW, if halb { 0.5 } else { 1.0 }, mal(staerke.clamp(0.0, 1.0)))),
        TextShadow::Below | TextShadow::Around => {
            let art = if stil == TextShadow::Below { crate::text_draw::TEXT_MODE_BELOW } else { crate::text_draw::TEXT_MODE_AROUND };
            Some((art, 0.0, mal(staerke.clamp(0.0, 100.0 / crate::text_draw::SOFT_SHADOW_FULL_PERCENT))))
        }
    }
}

// Das Rand-Viereck einer projizierten Glyphe (Weltschrift) nach der eingestellten Art, wie
// text_border_quad auf dem Bildschirm: Ecken in der Textebene, Atlas-UV, Art fuer font.wgsl und
// Farbe. Wie weit ein Bildschirmpixel in der Textebene und im Atlas reicht, sagen die
// projizierten Kanten des Glyphen (a, b, d: die Ecken (x0, y0), (x1, y0), (x0, y1) in
// Bildschirmpixeln); der Vanilla-Schatten rechnet in Schriftpixeln der Textebene (ein
// Schriftpixel nach rechts unten, auch schraeg gesehen).
fn projected_border(
    stil: TextShadow,
    staerke: f32,
    halb: bool,
    farbe: [f32; 4],
    [x0, y0, x1, y1]: [f32; 4],
    [u0, v0, u1, v1]: [f32; 4],
    (a, b, d): ((f32, f32), (f32, f32), (f32, f32)),
) -> Option<([f32; 4], [f32; 4], [f32; 4], [f32; 4])> {
    let (sx, sy) = ((b.0 - a.0).hypot(b.1 - a.1), (d.0 - a.0).hypot(d.1 - a.1));
    if sx <= 1e-3 || sy <= 1e-3 {
        return None;
    }
    // Bildschirmpixel je Einheit der Textebene, und Atlas-UV je Bildschirmpixel.
    let (kx, ky) = (sx / (x1 - x0), sy / (y1 - y0));
    let je_px = [(u1 - u0) / sx, (v1 - v0) / sy];
    // Raender (links, oben, rechts, unten) in Bildschirmpixeln.
    let (raender, art, wert, farbe) = match stil {
        TextShadow::Off => return None,
        TextShadow::Outline => {
            let dicke = staerke.clamp(0.5, 4.0);
            let r = dicke.ceil();
            ([r; 4], crate::text_draw::TEXT_MODE_OUTLINE_WORLD, dicke, farbe)
        }
        TextShadow::Vanilla => {
            // In Schriftpixeln: UV je Schriftpixel, Versatz 1 (bzw. 0,5).
            let v = if halb { 0.5 } else { 1.0 };
            let je_schrift = [(u1 - u0) / (x1 - x0), (v1 - v0) / (y1 - y0)];
            let farbe = [farbe[0], farbe[1], farbe[2], farbe[3] * staerke.clamp(0.0, 1.0)];
            return Some((
                [x0, y0, x1 + v, y1 + v],
                [u0, v0, u1 + je_schrift[0] * v, v1 + je_schrift[1] * v],
                [crate::text_draw::TEXT_MODE_SHADOW, je_schrift[0], je_schrift[1], v],
                farbe,
            ));
        }
        TextShadow::Below | TextShadow::Around => {
            let b = (kx * 0.9).max(1.5);
            let unten = stil == TextShadow::Below;
            let art = if unten { crate::text_draw::TEXT_MODE_BELOW } else { crate::text_draw::TEXT_MODE_AROUND };
            let grenze = 100.0 / crate::text_draw::SOFT_SHADOW_FULL_PERCENT;
            let farbe = [farbe[0], farbe[1], farbe[2], farbe[3] * staerke.clamp(0.0, grenze)];
            ([b, b, b, if unten { 2.0 * b } else { b }], art, b, farbe)
        }
    };
    let [l, o, r, u] = raender;
    Some((
        [x0 - l / kx, y0 - o / ky, x1 + r / kx, y1 + u / ky],
        [u0 - je_px[0] * l, v0 - je_px[1] * o, u1 + je_px[0] * r, v1 + je_px[1] * u],
        [art, je_px[0], je_px[1], wert],
        farbe,
    ))
}
