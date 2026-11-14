// Overlays, Hand-Debug-UI, UI oben drauf, Schriften und Fadenkreuz-Text.

use super::*;

impl GpuState {
    // Die Overlay-Ebene als Vertices, jede Gruppe um ihren Drehpunkt gedreht (s. UiOverlay).
    //
    // VOR dem Text der normalen UI aufrufen: shape_text merkt sich die Zeilen fuer die
    // Textauswahl und beginnt dabei jedes Mal von vorn -- so bleiben die Zeilen der normalen
    // UI stehen, und der (gedrehte) Overlay-Text ist nicht auswaehlbar.
    //
    // Dazu je Gruppe, wo ihre Flaechen und ihr Text liegen (relativ zum Anfang der Ebene):
    // gezeichnet wird Gruppe fuer Gruppe, erst Flaechen, dann Text. Sonst schiene der Text
    // einer Karte durch eine andere, die gerade ueber sie hinwegfaellt.
    pub(crate) fn overlay_vertices(&self, overlay: &UiOverlay, screen_px: (f32, f32)) -> OverlayVertices {
        let mut quads = Vec::new();
        let mut text = Vec::new();
        let mut sprites = Vec::new();
        let mut ranges = Vec::new();
        for g in &overlay.groups {
            let q0 = quads.len();
            let mut fade = None;
            for r in &g.rects {
                if let Some(f) = fade_marker_rect(r) {
                    fade = f;
                    continue;
                }
                match fade {
                    Some(f) => fade_quad_verts(&rect_verts(r, screen_px), f, screen_px.1, &mut quads),
                    None => quads.extend_from_slice(&rect_verts(r, screen_px)),
                }
            }
            let s0 = sprites.len();
            let laeufe = Self::shape_sprites(&g.sprites, screen_px, &mut sprites);
            let t0 = text.len();
            let mut glyphen = Vec::new();
            let schriften = self.shape_text(&g.texts, screen_px, &mut glyphen);
            text.extend(glyphen);
            if g.angle != 0.0 {
                for v in &mut quads[q0..] {
                    v.position = rotate_ndc(v.position, g.pivot, g.angle, screen_px);
                }
                for v in &mut text[t0..] {
                    v.position = rotate_ndc(v.position, g.pivot, g.angle, screen_px);
                }
                for v in &mut sprites[s0..] {
                    v.position = rotate_ndc(v.position, g.pivot, g.angle, screen_px);
                }
            }
            ranges.push(OverlayRange { quads: q0 as u32..quads.len() as u32, text: t0 as u32..text.len() as u32, sprites: laeufe, schriften, clip: g.clip });
        }
        OverlayVertices { quads, text, sprites, ranges }
    }

    // Das Oberste eines UI-Passes, in dieser Reihenfolge: der normale Text, die Overlay-Ebene
    // (ihre Flaechen, dann ihr Text) und zuletzt der Zeiger. `text`/`quads` sind je (Ende der
    // normalen UI, Ende insgesamt) in den Vertexpuffern, s. append_overlay.
    // Hand-Debug: Knoepfe und Werte rechts oben im Schwarzen samt Zeiger, direkt in die
    // Swapchain nach der Hand -- ins verkleinerte Bild gezeichnet laegen sie nie im Schwarzen.
    // Die Vertices kommen in dieselben Puffer hinter die des HUD (`ab`: Text, Flaechen).
    pub(crate) fn draw_hand_debug_ui(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, ui: (&[UiRect], &[UiText]), ab: (u32, u32), cursor: bool) {
        let screen_px = self.real_size();
        let (text_ab, quad_ab) = ab;
        let mut flaechen: Vec<Vertex> = ui.0.iter().flat_map(|r| rect_verts(r, screen_px)).collect();
        flaechen.truncate((MAX_UI_RECTS * VERTS_PER_QUAD).saturating_sub(quad_ab as usize));
        let mut glyphen = Vec::new();
        let schriften = self.shape_text(ui.1, screen_px, &mut glyphen);
        glyphen.truncate((MAX_UI_GLYPHS * TEXT_VERTS_PER_QUAD).saturating_sub(text_ab as usize));
        if !flaechen.is_empty() {
            let versatz = u64::from(quad_ab) * std::mem::size_of::<Vertex>() as u64;
            self.queue.write_buffer(&self.quad_vertex_buffer, versatz, bytemuck::cast_slice(&flaechen));
        }
        if !glyphen.is_empty() {
            let versatz = u64::from(text_ab) * std::mem::size_of::<TextVertex>() as u64;
            self.queue.write_buffer(&self.text_vertex_buffer, versatz, bytemuck::cast_slice(&glyphen));
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("hand debug ui pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if !flaechen.is_empty() {
            pass.set_pipeline(&self.quad_pipeline);
            pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
            pass.draw(quad_ab..quad_ab + flaechen.len() as u32, 0..1);
        }
        if !glyphen.is_empty() {
            pass.set_pipeline(&self.text_pipeline);
            pass.set_vertex_buffer(0, self.text_vertex_buffer.slice(..));
            pass.set_index_buffer(self.text_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            let n = glyphen.len() as u32;
            let mut von = 0;
            let mut kind = self.font_atlas.kind();
            for &(bis, naechste) in schriften.iter().chain(std::iter::once(&(n, kind))) {
                let bis = bis.min(n);
                if bis > von {
                    pass.set_bind_group(0, &self.hud_font(kind).bind_group, &[]);
                    pass.draw_indexed(text_index_range(text_ab + von..text_ab + bis), 0, 0..1);
                    von = bis;
                }
                kind = naechste;
            }
        }
        if cursor {
            pass.set_pipeline(&self.cursor_pipeline);
            pass.set_bind_group(0, &self.cursor_bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }

    pub(crate) fn draw_ui_top(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        text: (u32, u32),
        quads: (u32, u32),
        overlay: &[OverlayRange],
        // (Anfang der Overlay-Sprites, Ende insgesamt) im Sprite-Puffer, s. append_overlay_sprites.
        sprites: (u32, u32),
        cursor: bool,
        schriften: &[(u32, FontKind)],
    ) {
        if text.0 > 0 {
            // Abschnitt fuer Abschnitt mit dem Atlas seiner Schrift (s. shape_text).
            pass.set_pipeline(&self.text_pipeline);
            pass.set_vertex_buffer(0, self.text_vertex_buffer.slice(..));
            pass.set_index_buffer(self.text_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            let mut von = 0;
            let mut kind = self.font_atlas.kind();
            let clips = self.text_clips.borrow();
            let screen_px = self.real_size();
            for &(ab, naechste) in schriften.iter().chain(std::iter::once(&(text.0, kind))) {
                let ab = ab.min(text.0);
                if ab > von {
                    pass.set_bind_group(0, &self.hud_font(kind).bind_group, &[]);
                    draw_clipped(pass, von..ab, &clips, screen_px, |pass, r| pass.draw_indexed(text_index_range(r), 0, 0..1));
                    von = ab;
                }
                kind = naechste;
            }
        }
        // Gruppe fuer Gruppe (s. overlay_vertices). Die Bereiche sind relativ zum Anfang der
        // Ebene und werden auf das geklemmt, was append_overlay tatsaechlich hochgeladen hat.
        let klemmen = |r: &std::ops::Range<u32>, (von, bis): (u32, u32)| (von + r.start).min(bis)..(von + r.end).min(bis);
        let screen_px = self.real_size();
        for g in overlay {
            if !set_scissor(pass, g.clip, screen_px) {
                continue;
            }
            let q = klemmen(&g.quads, quads);
            if !q.is_empty() {
                pass.set_pipeline(&self.quad_pipeline);
                pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
                pass.draw(q, 0..1);
            }
            let laeufe: Vec<(SpriteAtlas, u32, u32)> = g
                .sprites
                .iter()
                .map(|&(atlas, start, count)| {
                    let r = klemmen(&(start..start + count), sprites);
                    (atlas, r.start, r.end - r.start)
                })
                .filter(|&(_, _, count)| count > 0)
                .collect();
            if !laeufe.is_empty() {
                pass.set_pipeline(&self.sprite_pipeline);
                pass.set_vertex_buffer(0, self.sprite_vertex_buffer.slice(..));
                self.draw_sprite_runs(pass, &laeufe);
            }
            let t = klemmen(&g.text, text);
            if !t.is_empty() {
                // Wie oben Abschnitt fuer Abschnitt mit dem Atlas seiner Schrift -- sonst
                // landen Glyphen einer anderen Schrift (kleine Beschriftungen in Outfit, s.
                // small_label_font) mit ihren UVs auf dem allgemeinen Atlas.
                pass.set_pipeline(&self.text_pipeline);
                pass.set_vertex_buffer(0, self.text_vertex_buffer.slice(..));
                pass.set_index_buffer(self.text_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                let anfang = text.0 + g.text.start;
                let mut von = t.start;
                let mut kind = self.font_atlas.kind();
                for &(ab, naechste) in g.schriften.iter().chain(std::iter::once(&(u32::MAX - anfang, kind))) {
                    let ab = (anfang + ab).clamp(t.start, t.end);
                    if ab > von {
                        pass.set_bind_group(0, &self.hud_font(kind).bind_group, &[]);
                        pass.draw_indexed(text_index_range(von..ab), 0, 0..1);
                        von = ab;
                    }
                    kind = naechste;
                }
            }
        }
        set_scissor(pass, None, screen_px);
        if cursor {
            pass.set_pipeline(&self.cursor_pipeline);
            pass.set_bind_group(0, &self.cursor_bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }

    // Die weiteren Font-Atlanten bereitstellen, die Anzeigen oder Bereiche statt der
    // allgemeinen Schrift wollen; nicht mehr gewollte fallen weg. Gebaut wird nur beim ersten
    // Bedarf -- eine TTF zu rastern kostet einen Moment.
    pub(crate) fn ensure_fonts(&mut self, installation: &mut assets::MinecraftInstallation, wanted: &[FontKind]) {
        let allgemein = self.font_atlas.kind();
        let gewollt = |k: FontKind| k != allgemein && wanted.contains(&k);
        self.extra_fonts.retain(|a| gewollt(a.kind()));
        // Nicht mehr gewollt: der Thread rechnet zu Ende, sein Ergebnis verfaellt.
        self.pending_fonts.retain(|p| gewollt(p.kind()));
        for &kind in wanted {
            if kind == allgemein || self.extra_fonts.iter().any(|a| a.kind() == kind) || self.pending_fonts.iter().any(|p| p.kind() == kind) {
                continue;
            }
            match FontAtlas::build_in_background(installation, kind) {
                Ok(p) => self.pending_fonts.push(p),
                Err(e) => eprintln!("[font] Zweitschrift {} nicht geladen: {e}", kind.label()),
            }
        }
        // Hochladen hoechstens einen je Bild: ein Atlas sind mit Mip-Stufen 45 bis 70 MB.
        let mut fertig = None;
        for (i, p) in self.pending_fonts.iter().enumerate() {
            match p.try_finish(&self.device, &self.queue, self.font_atlas.bind_group_layout.clone()) {
                Ok(None) => continue,
                Ok(Some(atlas)) => fertig = Some((i, Some(atlas))),
                Err(_) => {
                    eprintln!("[font] Zweitschrift {} nicht geladen: Thread ohne Ergebnis beendet", p.kind().label());
                    fertig = Some((i, None));
                }
            }
            break;
        }
        if let Some((i, atlas)) = fertig {
            self.pending_fonts.remove(i);
            if let Some(mut atlas) = atlas {
                atlas.set_pack(&self.device, self.pack_glyphs.clone());
                self.extra_fonts.push(atlas);
            }
        }
    }

    // Neue Resourcepack-Schriften in alle Atlanten (nach einem Pack-Wechsel).
    pub(crate) fn set_pack_fonts(&mut self, fonts: assets::PackFonts) {
        let cache = Some(Arc::new(font_atlas::PackGlyphCache::new(&self.device, self.queue.clone(), fonts)));
        self.font_atlas.set_pack(&self.device, cache.clone());
        for a in &mut self.extra_fonts {
            a.set_pack(&self.device, cache.clone());
        }
        self.pack_glyphs = cache;
    }

    // Der Atlas in Schrift `kind`, solange er nicht geladen ist der gewoehnliche.
    pub(crate) fn hud_font(&self, kind: FontKind) -> &FontAtlas {
        if kind == self.font_atlas.kind() {
            return &self.font_atlas;
        }
        self.extra_fonts.iter().find(|a| a.kind() == kind).unwrap_or(&self.font_atlas)
    }

    // Der Atlas, mit dem gerade gemessen und gesetzt wird (s. layout_font).
    pub(crate) fn layout_atlas(&self) -> &FontAtlas {
        self.layout_font.get().map_or(&self.font_atlas, |k| self.hud_font(k))
    }

    pub(crate) fn set_layout_font(&self, font: Option<FontKind>) {
        self.layout_font.set(font);
    }

    // Kleine Beschriftungen (TOPBAR_SUB_SCALE) nie in der Minecraft-Schrift: 1,5-fach zerlegt
    // sie die Font-Pixel ungleich (mal ein, mal zwei Bildschirmpixel), das sieht fett und
    // zerhackt aus. Dort Outfit, fuers Messen wie fuers Zeichnen.
    pub(crate) fn small_label_font(&self, scale: f32) -> Option<LayoutFontScope<'_>> {
        (scale == TOPBAR_SUB_SCALE && self.layout_atlas().kind() == FontKind::Minecraft).then(|| {
            let scope = LayoutFontScope { gpu: self, vorher: self.layout_font.get(), ruhig_vorher: self.plain_numbers.get() };
            self.set_layout_font(Some(FontKind::Outfit));
            scope
        })
    }

    // Eine HUD-Zahl als Glyphen-Vierecke fuer den Fadenkreuz-Pass, um ihre Mitte gesetzt.
    // Weiss wird immer umrandet; in Fadenkreuz-Farbe nur, wenn das Kreuz eine Kontur hat.
    pub(crate) fn crosshair_glyph_vertices(&self, atlas: &FontAtlas, t: &CrosshairText, crosshair_outline: bool, screen_px: (f32, f32)) -> Vec<CrosshairVertex> {
        let scale = t.scale;
        let breite: f32 = t.text.chars().filter_map(|c| crosshair_advance(atlas, c, t.stable_digits)).map(|(platz, _)| platz * scale).sum();
        let hoehe = atlas.cell_height_px() * scale;
        let x0 = (t.center_px.0 - breite / 2.0).round();
        let top = (t.center_px.1 - hoehe / 2.0).round();
        let baseline = top + atlas.ascent_px() * scale;
        // Die Glyphen einmal setzen, danach je Lage (Kontur-Versaetze, Fuellung) ausgeben.
        let mut quads = Vec::new();
        let mut pen = x0;
        for ch in t.text.chars() {
            let Some((platz, eigen)) = crosshair_advance(atlas, ch, t.stable_digits) else { continue };
            // Mittig im Platz (ohne stabile Ziffern ist der Platz der eigene Vorschub).
            let hier = pen + (platz - eigen) * scale / 2.0;
            if let Some(p) = atlas.place(ch, scale).filter(|p| p.size[0] > 0.0 && p.size[1] > 0.0) {
                let (p, x) = atlas.snap_x(ch, scale, false, p, hier);
                let (ox, oy) = if p.snap { (x, baseline.round()) } else { (hier, baseline) };
                quads.push((ox + p.offset[0], oy + p.offset[1], p.size, p.uv_min, p.uv_max));
            }
            pen += platz * scale;
        }
        let konturiert = t.white || t.yellow || crosshair_outline;
        let d = (scale / 2.0).round().max(1.0);
        const RICHTUNGEN: [(f32, f32); 8] = [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)];
        let fuellung = if t.yellow {
            CROSSHAIR_KIND_GLYPH_YELLOW
        } else if t.white {
            CROSSHAIR_KIND_GLYPH_WHITE
        } else {
            CROSSHAIR_KIND_GLYPH
        };
        let lagen = RICHTUNGEN
            .iter()
            .filter(|_| konturiert)
            .map(|&(dx, dy)| (dx * d, dy * d, CROSSHAIR_KIND_GLYPH_BLACK))
            .chain(std::iter::once((0.0, 0.0, fuellung)));
        let mut out = Vec::new();
        for (dx, dy, kind) in lagen {
            for &(x, y, size, uv0, uv1) in &quads {
                let (x0, y0, x1, y1) = (x + dx, y + dy, x + dx + size[0], y + dy + size[1]);
                let v = |px: f32, py: f32, gu: f32, gv: f32| CrosshairVertex {
                    position: pixel_to_ndc(px, py, screen_px),
                    uv: [px / screen_px.0, py / screen_px.1],
                    glyph_uv: [gu, gv],
                    kind,
                    alpha: t.alpha,
                    circle: [t.fade_slot as f32, 0.0],
                };
                out.extend_from_slice(&[
                    v(x0, y0, uv0[0], uv0[1]),
                    v(x1, y0, uv1[0], uv0[1]),
                    v(x1, y1, uv1[0], uv1[1]),
                    v(x0, y0, uv0[0], uv0[1]),
                    v(x1, y1, uv1[0], uv1[1]),
                    v(x0, y1, uv0[0], uv1[1]),
                ]);
            }
        }
        out
    }
}
