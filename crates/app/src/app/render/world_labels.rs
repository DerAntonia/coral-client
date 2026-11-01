// Namensschilder, Schildtexte und Display-Text in der Welt.

use super::*;

impl App {
    // Beschriftungen, die IN der Welt haengen: text_display-Entities und die Namensschilder
    // von Entities mit eigenem Namen (Entity.CUSTOM_NAME + NAME_VISIBLE).
    //
    // Beides zusammen ist das, was Server als "Hologramm" ausliefern: entweder ein
    // text_display oder -- klassisch und immer noch verbreitet -- ein unsichtbarer
    // Armorstand mit dauerhaft eingeblendetem Namen. Vorher zeigte dieser Client davon
    // nichts: der Text fehlte ganz, und der Traeger stand als orangefarbener Kasten da.
    //
    // SPIELERNAMEN laufen seit 2026-09-15 durch DENSELBEN Weg (s. world_label). Vorher gab es
    // dafuer einen zweiten Zeichenpfad, der die Schilder in fester Groesse und immer mit
    // Sichtlinien-Pruefung zeichnete -- beides wich von Vanilla ab, und der doppelte Pfad war
    // der Grund, warum die Regeln dort (unsichtbar, schleichend, Sichtweite) fehlten.
    //
    // Was daran NICHT Vanilla ist und bewusst so bleibt:
    //   - Die Schrift steht im Bildschirmraum, nicht als Flaeche in der Welt. Verdeckung
    //     muss deshalb wie bei den Namensschildern von Hand geprueft werden (s. sicht_frei),
    //     und die Warp-Projektionen erreichen sie nicht (derselbe Vorbehalt wie dort).
    //   - Der ganzzahlige Faktor springt in Stufen, statt stufenlos zu schrumpfen. Eine
    //     Bruchskalierung zerlegt einen Bitmap-Font sichtbar; die Stufe ist das kleinere
    //     Uebel.
    //
    // TEXT-DISPLAYS gehen seit 2026-09-19 einen eigenen Weg (s. build_display_text): Vanilla
    // legt sie mit Billboard-Modus und Drehungen als echte Flaeche in die Welt -- ein FIXED-
    // Display (der Vorgabewert!) steht wie ein Schild im Raum, statt sich zur Kamera zu
    // drehen. Nur Namensschilder stehen in Vanilla immer parallel zum Bildschirm
    // (LabelCommandRenderer: `multiply(cameraState.orientation)`), und genau das ist dieser
    // Bildschirmraum-Weg.
    //
    // Schrift und Rand aus dem Bereich Nametags -- fuer die Namensschilder (UiText) wie fuer die
    // projizierten Text-Displays (world_label_glyphs, gezeichnet in gpu.label_font).
    pub(crate) fn build_world_labels(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        gpu.world_label_glyphs.borrow_mut().clear();
        gpu.world_label_quads.borrow_mut().clear();
        gpu.label_font.set(self.settings.font_of(ShadowArea::Nametags));
        self.begin_text_area(gpu, texts, ShadowArea::Nametags);
        self.build_world_labels_in(gpu, rects, texts);
        Self::end_text_area(gpu, texts);
    }

    pub(crate) fn build_world_labels_in(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        if self.entities.is_empty() {
            return;
        }
        let (sw, sh) = gpu.size();
        let aspect = sw / sh.max(1.0);
        let view_proj = self.camera.view_proj_matrix(aspect);
        let eye = self.camera.view_position();
        let progress = self.tick_progress();
        // "Oben" aus Sicht der Kamera, in Weltkoordinaten: die Y-Achse, um den Blickanteil
        // bereinigt. Damit wird gleich gemessen, wie viele Bildschirmpixel ein Weltblock an
        // dieser Stelle hoch ist -- daraus ergibt sich der Schriftfaktor.
        let forward = self.camera.view_forward();
        let up = {
            let v = glam::Vec3::Y - forward * forward.dot(glam::Vec3::Y);
            if v.length_squared() > 1e-6 {
                v.normalize()
            } else {
                // Senkrecht nach oben/unten geschaut: dann taugt Y nicht mehr als Bezug.
                self.camera.right().cross(forward).normalize()
            }
        };
        // Schrifthoehe und Zeilenabstand in SCHRIFTPIXELN (also beim Faktor 1).
        let font_line = gpu.text_height(1.0);
        // s. label_debug_log -- nur gesammelt, wenn die Umgebungsvariable gesetzt ist.
        let mut debug_zeilen: Vec<String> = Vec::new();
        let debug = label_debug_an();
        let line_pitch = font_line + WORLD_LABEL_LINE_GAP;

        for (&_id, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) {
                continue;
            }
            // Der eigene Koerper traegt in Vanilla nie eine Beschriftung.
            if Some(e.uuid) == self.own_uuid {
                continue;
            }
            let Some(label) = self.world_label(_id, e, progress) else { continue };
            // Skalierung 0 (oder negativ) heisst: der Server hat die Beschriftung bewusst
            // unsichtbar gemacht.
            if label.world_scale <= 0.0 {
                continue;
            }
            if eye.distance_squared(label.anchor) > label.max_distance * label.max_distance {
                continue;
            }
            if let Some(basis) = label.display_matrix {
                self.build_display_text(gpu, &label, basis, view_proj, (sw, sh));
                continue;
            }
            // Augenrelativ, genau wie die Weltgeometrie (s. build_entity_vertices).
            let rel = (label.anchor - self.camera.position).as_vec3();
            let clip = view_proj * glam::Vec4::new(rel.x, rel.y, rel.z, 1.0);
            if clip.w <= NAMETAG_MIN_CLIP_W {
                continue;
            }
            let ndc = (clip.x / clip.w, clip.y / clip.w);
            if !(-1.0..=1.0).contains(&ndc.0) || !(-1.0..=1.0).contains(&ndc.1) {
                continue;
            }
            // Die Sichtlinien-Pruefung ERST hier, nach der Projektion: sie tastet die
            // Strecke bis zum Anker in 0.25-Bloecke-Schritten ab und ist damit das mit
            // Abstand Teuerste an dieser Schleife. Auf einem Server mit vielen Hologrammen
            // ist der grosse Teil davon ohnehin ausserhalb des Bildes -- die billigen
            // Abweisungen gehoeren davor.
            //
            // Ein Text mit "see through"-Flag darf durch Waende gelesen werden -- Vanilla
            // zeichnet ihn dann ohne Tiefentest.
            if !label.see_through && !self.sicht_frei(eye, label.anchor) {
                continue;
            }
            // Wie viele Bildschirmpixel ist EIN Weltblock an dieser Tiefe hoch? Gemessen
            // statt aus der Projektionsmatrix zurueckgerechnet: der Aufbau der Matrix (FOV,
            // Seitenverhaeltnis, Vorzeichen der Y-Achse) muss hier dann nirgends doppelt
            // stehen. `up` steht senkrecht auf der Blickrichtung, der Tiefenwert w bleibt
            // dadurch derselbe -- die Messung ist exakt, nicht genaehert.
            let clip_up = view_proj * glam::Vec4::new(rel.x + up.x, rel.y + up.y, rel.z + up.z, 1.0);
            if clip_up.w <= NAMETAG_MIN_CLIP_W {
                continue;
            }
            let px_pro_block = ((clip_up.y / clip_up.w - ndc.1) * 0.5 * sh).abs();
            let faktor = WORLD_LABEL_WORLD_SCALE * label.world_scale * px_pro_block;
            if !faktor.is_finite() {
                continue;
            }
            // STUFENLOS, nicht auf ganze Stufen gerundet: gerundet sprang die Schrift beim
            // Gehen sichtbar in Groessen.
            // Der Preis ist eine leicht unregelmaessige Pixelgroesse in der Ferne; das ist
            // erheblich weniger stoerend als das Springen, und in Vanilla schrumpft die
            // Schrift ebenfalls stufenlos.
            //
            // GEDECKELT, nicht weggeworfen: eine Beschriftung, die in der Ferne einfach
            // verschwindet, sieht wie ein Fehler aus. Vanilla zeichnet sie weiter, nur immer
            // kleiner. Wo Schluss ist, entscheidet allein die Sichtweite
            // (s. label.max_distance).
            let scale = faktor.clamp(WORLD_LABEL_MIN_SCALE, WORLD_LABEL_MAX_SCALE);

            // Zeilen trennen, umbrechen und vermessen -- alles in Schriftpixeln, erst danach
            // mal `scale` in Bildschirmpixel.
            let zeilen = wrap_label_lines(&split_label_lines(&self.ego_spans(&label.spans)), label.line_width, |t| gpu.measure_text(t, 1.0));
            if zeilen.is_empty() {
                continue;
            }
            let breiten: Vec<f32> = zeilen.iter().map(|z| z.iter().map(|s| gpu.measure_text(&s.text, 1.0)).sum()).collect();
            let block_breite = breiten.iter().copied().fold(0.0f32, f32::max);
            // Vanillas `n`: die letzte Zeile bekommt den Zeilenabstand nicht mehr dazu.
            let block_hoehe = zeilen.len() as f32 * line_pitch - WORLD_LABEL_LINE_GAP;

            if debug {
                let text: String = zeilen.iter().flat_map(|z| z.iter().map(|s| s.text.clone())).collect();
                let typ = world::entity_type_name(e.entity_type).unwrap_or("?");
                let quelle = if e.display.is_some() {
                    "Display"
                } else if world::is_player(e.entity_type) {
                    "Spieler"
                } else {
                    "Custom-Name"
                };
                let team = self
                    .player_names
                    .get(&e.uuid)
                    .and_then(|n| self.sb_entry_team.get(n)).map_or_else(|| "-".to_string(), |t| {
                        let sicht = self.sb_teams.get(t).map_or(TEAM_VISIBILITY_ALWAYS, |i| i.name_tag_visibility);
                        format!("{t}(sicht={sicht})")
                    });
                debug_zeilen.push(format!(
                    "{quelle} id={_id} typ={typ} y={:.2} dist={:.1} durchwand={} team={team} text={text:?}",
                    label.anchor.y,
                    eye.distance(label.anchor),
                    label.see_through,
                ));
            }
            let (sx, sy) = ((ndc.0 * 0.5 + 0.5) * sw, (0.5 - ndc.1 * 0.5) * sh);
            let links = (sx - block_breite * scale / 2.0).round();
            // Ein Text-Display waechst NACH OBEN vom Ankerpunkt weg (Vanillas
            // `translate(1 - m/2, -n, 0)` setzt die Unterkante auf den Ursprung); ein
            // Namensschild sitzt dagegen mittig auf seinem Ankerpunkt.
            let block_px = block_hoehe * scale;
            let oben = if label.above_anchor { (sy - block_px).round() } else { (sy - block_px / 2.0).round() };

            // Hintergrundflaeche, ein Schriftpixel Luft ringsum (Vanilla: -1 bis m bzw. n).
            let bg = argb_to_rgba(label.background);
            if bg[3] > 0.0 {
                rects.push(UiRect {
                    rect: Rect {
                        x: links - scale,
                        y: oben - scale,
                        w: (block_breite + 1.0) * scale,
                        h: (block_hoehe + 1.0) * scale,
                    },
                    color: bg,
                    ..UiRect::FLAT
                });
            }

            let deckkraft = f32::from(label.opacity) / 255.0;
            for (i, zeile) in zeilen.iter().enumerate() {
                // Ausrichtung innerhalb des Blocks (TextDisplayEntity.TextAlignment).
                let versatz = match label.align {
                    LabelAlign::Left => 0.0,
                    LabelAlign::Right => block_breite - breiten[i],
                    LabelAlign::Center => (block_breite - breiten[i]) / 2.0,
                };
                let mut x = links + versatz * scale;
                let y = oben + i as f32 * line_pitch * scale;
                for span in zeile {
                    let mut color = span.color.map_or(COLOR_WHITE, rgb_to_rgba);
                    color[3] *= deckkraft;
                    let breite = gpu.measure_text(&span.text, 1.0) * scale;
                    push_span_shadow(texts, span.shadow);
                    texts.push(UiText { text: span.text.clone(), x: x.round(), y, color, scale });
                    x += breite;
                }
            }
        }
        if debug {
            label_debug_log(&debug_zeilen);
        }
    }

    // Der Text aller nahen Schilder als Flaeche in der Welt -- derselbe Weg wie beim
    // Text-Display (s. build_display_text): Vanillas Matrixkette bis in die Textebene, jede
    // Glyphe auf den Bildschirm projiziert. Nach AbstractSignBlockEntityRenderer.renderText:
    //   [Rueckseite: rotY(180)], translate(Textversatz), scale(f, -f, f),
    //   f = 1/64 * Textgroesse (2/3 stehend, 1 haengend), Zeile i bei y = i * h - 2 * h.
    //
    // Gezeichnet wird im Welt-Durchgang mit Tiefentest (s. GpuState::world_text_pipeline):
    // Entities und Bloecke davor verdecken den Text, und wie in Vanilla bleibt er bei
    // verstecktem HUD (F1) und unter offenen Fenstern stehen. Schrift und Rand aus dem Bereich
    // Schilder.
    pub(crate) fn build_sign_texts(&self, gpu: &GpuState) {
        const SIGN_TEXT_DISTANCE: f64 = 32.0;
        gpu.sign_glyphs.borrow_mut().clear();
        let _schrift = self.area_font_scope(gpu, ShadowArea::Signs);
        gpu.sign_font.set(self.settings.font_of(ShadowArea::Signs));
        let (sw, sh) = gpu.size();
        let view_proj = self.camera.view_proj_matrix(sw / sh.max(1.0));
        let auge = self.camera.view_position();
        // Die Seite im Editor zeichnet unten der Editor selbst, mit dem Text, wie er gerade
        // getippt ist.
        let editiert = self.sign_edit_in_world(gpu).and(self.sign_edit.as_ref());
        for (&pos, text) in &self.signs {
            let mitte = DVec3::new(f64::from(pos.0) + 0.5, f64::from(pos.1) + 0.5, f64::from(pos.2) + 0.5);
            if !self.block_column_drawn(pos) || (mitte - auge).length_squared() > SIGN_TEXT_DISTANCE * SIGN_TEXT_DISTANCE {
                continue;
            }
            for (seite, vorn) in [(&text.front, true), (&text.back, false)] {
                if seite.lines.iter().all(|z| z.iter().all(|s| s.text.is_empty())) || editiert.is_some_and(|e| e.pos == pos && e.front == vorn) {
                    continue;
                }
                let Some((frame, zeile_h)) = self.sign_side_frame(pos, vorn) else { continue };
                // Verdeckung macht der Tiefentest -- nur ohne ihn (Shader-Pfad) der Strahl.
                if self.sign_side_visible(frame, view_proj, zeile_h, (sw, sh), gpu.use_shader_path) {
                    self.push_sign_side(gpu, seite, frame, view_proj, zeile_h, (sw, sh));
                }
            }
        }
        if let Some(edit) = editiert {
            self.push_sign_edit_in_world(gpu, edit, view_proj, (sw, sh));
        }
    }

    // Vanillas Matrixkette bis in die Textebene einer Schildseite (s. build_sign_texts) und
    // die Zeilenhoehe dort; None, wenn an `pos` kein Schild steht.
    pub(crate) fn sign_side_frame(&self, pos: (i32, i32, i32), vorn: bool) -> Option<(glam::Mat4, f32)> {
        use world::entity_model::{Affine, SignKind};
        let (_, art, grad) = self.sign_at(pos)?;
        let haengend = matches!(art, SignKind::Hanging(_));
        let rel = (DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)) - self.camera.position).as_vec3();
        let mut b = world::entity_model::sign_base([rel.x, rel.y, rel.z], art, grad);
        let (versatz, f, zeile_h) = if haengend { ([0.0, -0.32, 0.073], 1.0 / 64.0, 9.0) } else { ([0.0, 1.0 / 3.0, 0.046_666_667], (1.0 / 64.0) * (2.0 / 3.0), 10.0) };
        if !vorn {
            b = b.then(Affine::rotation_y_deg(180.0));
        }
        Some((mat4_from_affine(&b.then(Affine::translation(versatz)).then(Affine::scale([f, -f, f]))), zeile_h))
    }

    // Sieht man diese Schildseite gerade: von vorn, im Bild und (mit `verdeckung`) nicht hinter
    // Bloecken?
    pub(crate) fn sign_side_visible(&self, frame: glam::Mat4, view_proj: glam::Mat4, zeile_h: f32, screen: (f32, f32), verdeckung: bool) -> bool {
        let (sw, sh) = screen;
        let project = sign_projector(view_proj * frame, screen);
        let oben = -2.0 * zeile_h;
        // Von hinten unsichtbar (dieselbe Pruefung wie beim Text-Display).
        let [Some(a), Some(b), Some(c)] = [project(-45.0, oben), project(45.0, oben), project(45.0, -oben)] else { return false };
        if (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0) <= 0.0 {
            return false;
        }
        if [a, b, c].iter().all(|p| p.0 < 0.0) || [a, b, c].iter().all(|p| p.0 > sw) || [a, b, c].iter().all(|p| p.1 < 0.0) || [a, b, c].iter().all(|p| p.1 > sh) {
            return false;
        }
        if !verdeckung {
            return true;
        }
        // Verdeckt? Gemessen an einem Punkt knapp vor der Textmitte -- die Mitte selbst liegt
        // im Umriss des eigenen Schildes.
        let m = frame * glam::Vec4::new(0.0, 0.0, 0.0, 1.0);
        let welt = self.camera.position + DVec3::new(f64::from(m.x), f64::from(m.y), f64::from(m.z));
        let auge = self.camera.view_position();
        let vor = welt + (auge - welt).normalize_or_zero() * 0.3;
        self.sicht_frei(auge, vor)
    }

    // Schreibt man direkt auf dem Schild in der Welt? Ja, solange man die bearbeitete Seite
    // sieht; sonst (Schild im Ruecken, verdeckt, HUD versteckt) das Editor-Fenster wie Vanilla.
    // Liefert Matrix und Zeilenhoehe der Seite.
    pub(crate) fn sign_edit_in_world(&self, gpu: &GpuState) -> Option<(glam::Mat4, f32)> {
        let edit = self.sign_edit.as_ref()?;
        if self.mode != AppMode::World || self.hud_hidden || !self.settings.sign_edit_in_world {
            return None;
        }
        let (sw, sh) = gpu.size();
        let view_proj = self.camera.view_proj_matrix(sw / sh.max(1.0));
        let (frame, zeile_h) = self.sign_side_frame(edit.pos, edit.front)?;
        self.sign_side_visible(frame, view_proj, zeile_h, (sw, sh), true).then_some((frame, zeile_h))
    }

    // Die Seite im Editor auf dem Schild selbst: der getippte Text und der Schreibcursor am
    // Ende der aktuellen Zeile. Der Cursor gleitet wie in den Eingabefeldern (s. push_caret),
    // hier auch beim Zeilenwechsel, und blinkt nach jeder Bewegung von vorn.
    pub(crate) fn push_sign_edit_in_world(&self, gpu: &GpuState, edit: &SignEdit, view_proj: glam::Mat4, screen: (f32, f32)) {
        let Some((frame, zeile_h)) = self.sign_side_frame(edit.pos, edit.front) else { return };
        let glowing = self.signs.get(&edit.pos).is_some_and(|t| if edit.front { t.front.glowing } else { t.back.glowing });
        let seite = protocol::sign::SignSide {
            lines: std::array::from_fn(|i| {
                let t = &edit.lines[i];
                if t.is_empty() { Vec::new() } else { vec![protocol::nbt::TextSpan { text: t.clone(), ..Default::default() }] }
            }),
            color: edit.color,
            glowing,
        };
        self.push_sign_side(gpu, &seite, frame, view_proj, zeile_h, screen);

        let jetzt = Instant::now();
        let ziel = (gpu.measure_text(&edit.lines[edit.row], 1.0) / 2.0, edit.row as f32 * zeile_h - 2.0 * zeile_h);
        let c = match edit.caret.get() {
            Some(c) if c.to == ziel => c,
            Some(c) => SignCaret { from: c.at(jetzt), to: ziel, start: jetzt },
            None => SignCaret { from: ziel, to: ziel, start: jetzt },
        };
        edit.caret.set(Some(c));
        let (x, top) = c.at(jetzt);
        let h = gpu.text_height(1.0);
        let project = sign_projector(view_proj * frame, screen);
        let [Some(a), Some(b), Some(cc), Some(d)] = [project(x + 0.5, top - 1.0), project(x + 1.5, top - 1.0), project(x + 1.5, top + h), project(x + 0.5, top + h)]
        else {
            return;
        };
        let mut farbe = rgb_to_rgba(sign_text_color(edit.color, glowing));
        farbe[3] *= caret_alpha(jetzt.duration_since(c.start));
        let v = |p: (f32, f32)| Vertex { position: pixel_to_ndc(p.0, p.1, screen), color: farbe, ..Vertex::PLAIN };
        gpu.world_label_quads.borrow_mut().extend_from_slice(&[v(a), v(b), v(cc), v(a), v(cc), v(d)]);
    }

    // Eine Schildseite in gpu.sign_glyphs, mit der Tiefe jedes Punktes fuer den Welt-Durchgang.
    // Rand: wie bei der Weltschrift, ausser der Bereich Schilder hat ihn ausgeschaltet.
    pub(crate) fn push_sign_side(&self, gpu: &GpuState, seite: &protocol::sign::SignSide, frame: glam::Mat4, view_proj: glam::Mat4, zeile_h: f32, screen: (f32, f32)) {
        let clip = view_proj * frame;
        let project = sign_projector(clip, screen);
        let tiefe = |x: f32, y: f32| {
            let c = clip * glam::Vec4::new(x, y, 0.0, 1.0);
            c.z / c.w
        };
        let oben = -2.0 * zeile_h;
        let raster = match (project(0.0, 0.0), project(1.0, 0.0)) {
            (Some(p), Some(q)) => ((q.0 - p.0).hypot(q.1 - p.1)).clamp(1.0, WORLD_LABEL_MAX_SCALE),
            _ => 1.0,
        };
        let vorgabe = sign_text_color(seite.color, seite.glowing);
        let rand_an = self.settings.shadow_of(ShadowArea::Signs) != Some(TextShadow::Off);
        let mut glyphen = gpu.sign_glyphs.borrow_mut();
        for (i, zeile) in seite.lines.iter().enumerate() {
            let breite: f32 = zeile.iter().map(|s| gpu.measure_text(&s.text, 1.0)).sum();
            let mut x = -breite / 2.0;
            let top = i as f32 * zeile_h + oben;
            for span in zeile {
                let color = rgb_to_rgba(span.color.unwrap_or(vorgabe));
                let rand = gpu.world_text_border(color, span.shadow, rand_an);
                x += gpu.shape_projected_text(&span.text, x, top, color, rand, raster, &project, Some(&tiefe), screen, &mut glyphen);
            }
        }
        let ganz = glyphen.len().min(MAX_SIGN_TEXT_VERTICES) / VERTS_PER_QUAD * VERTS_PER_QUAD;
        glyphen.truncate(ganz);
    }

    // Kamerawinkel wie Vanillas Camera.getYaw/-Pitch: in der Frontansicht umgedreht (s.
    // Camera::view_forward). camera.pitch zaehlt nach OBEN positiv, Minecraft nach unten --
    // daher das Vorzeichen.
    pub(crate) fn vanilla_camera_angles(&self) -> (f32, f32) {
        let (yaw, pitch) = self.camera.view_angles();
        (yaw, -pitch)
    }

    // Ein Text-Display als Flaeche in der Welt: Vanillas Matrixkette (Entity-Position,
    // Billboard, AffineTransformation, Text-Skalierung -- s. WorldLabel::display_matrix und
    // display_text_frame), jede Glyphe und der Hintergrund einzeln auf den Bildschirm
    // projiziert. Wie Vanilla nur von VORN sichtbar: von hinten zeigt ein Display nichts.
    pub(crate) fn build_display_text(&self, gpu: &GpuState, label: &WorldLabel, basis: glam::Mat4, view_proj: glam::Mat4, screen: (f32, f32)) {
        let line_pitch = gpu.text_height(1.0) + WORLD_LABEL_LINE_GAP;
        let zeilen = wrap_label_lines(&split_label_lines(&self.ego_spans(&label.spans)), label.line_width, |t| gpu.measure_text(t, 1.0));
        if zeilen.is_empty() {
            return;
        }
        let breiten: Vec<f32> = zeilen.iter().map(|z| z.iter().map(|s| gpu.measure_text(&s.text, 1.0)).sum()).collect();
        let m = breiten.iter().copied().fold(0.0f32, f32::max);
        let n = zeilen.len() as f32 * line_pitch - WORLD_LABEL_LINE_GAP;
        let frame = basis * display_text_frame(m, n);
        let clip = view_proj * frame;
        let (sw, sh) = screen;
        let project = |x: f32, y: f32| -> Option<(f32, f32)> {
            let c = clip * glam::Vec4::new(x, y, 0.0, 1.0);
            (c.w > NAMETAG_MIN_CLIP_W).then(|| ((c.x / c.w * 0.5 + 0.5) * sw, (0.5 - c.y / c.w * 0.5) * sh))
        };

        // Die Hintergrundflaeche (Vanilla: -1..m, -1..n) entscheidet ueber Sichtbarkeit.
        let ecken = [project(-1.0, -1.0), project(m, -1.0), project(m, n), project(-1.0, n)];
        let [Some(a), Some(b), Some(c), Some(d)] = ecken else { return };
        // Rueckseite: in der Textebene laufen die Ecken im Uhrzeigersinn (y nach unten);
        // auf dem Bildschirm gespiegelt heisst, man schaut von hinten.
        let kreuz = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if kreuz <= 0.0 {
            return;
        }
        let (xs, ys) = ([a.0, b.0, c.0, d.0], [a.1, b.1, c.1, d.1]);
        if xs.iter().all(|&x| x < 0.0) || xs.iter().all(|&x| x > sw) || ys.iter().all(|&y| y < 0.0) || ys.iter().all(|&y| y > sh) {
            return;
        }
        // Verdeckung wie bei den Namensschildern, gemessen an der Mitte der Flaeche.
        if !label.see_through {
            let mitte = frame * glam::Vec4::new((m - 1.0) / 2.0, (n - 1.0) / 2.0, 0.0, 1.0);
            let welt = self.camera.position + glam::DVec3::new(f64::from(mitte.x), f64::from(mitte.y), f64::from(mitte.z));
            if !self.sicht_frei(self.camera.view_position(), welt) {
                return;
            }
        }

        let bg = argb_to_rgba(label.background);
        if bg[3] > 0.0 {
            let v = |p: (f32, f32)| Vertex { position: pixel_to_ndc(p.0, p.1, screen), color: bg, ..Vertex::PLAIN };
            gpu.world_label_quads.borrow_mut().extend_from_slice(&[v(a), v(b), v(c), v(a), v(c), v(d)]);
        }

        // Rasterstufe der Glyphen: wie viele Bildschirmpixel ein Schriftpixel etwa breit ist.
        let raster = match (project(m / 2.0, n / 2.0), project(m / 2.0 + 1.0, n / 2.0)) {
            (Some(p), Some(q)) => ((q.0 - p.0).hypot(q.1 - p.1)).clamp(1.0, WORLD_LABEL_MAX_SCALE),
            _ => 1.0,
        };
        let deckkraft = f32::from(label.opacity) / 255.0;
        let mut glyphen = gpu.world_label_glyphs.borrow_mut();
        for (i, zeile) in zeilen.iter().enumerate() {
            let mut x = match label.align {
                LabelAlign::Left => 0.0,
                LabelAlign::Right => m - breiten[i],
                LabelAlign::Center => (m - breiten[i]) / 2.0,
            };
            let top = i as f32 * line_pitch;
            for span in zeile {
                let mut color = span.color.map_or(COLOR_WHITE, rgb_to_rgba);
                color[3] *= deckkraft;
                let rand = gpu.world_text_border(color, span.shadow, label.shadow);
                x += gpu.shape_projected_text(&span.text, x, top, color, rand, raster, &project, None, screen, &mut glyphen);
            }
        }
    }

    // Was an diesem Entity als Weltbeschriftung haengt -- oder None, wenn nichts.
    pub(crate) fn world_label<'a>(&self, id: i32, e: &'a world::Entity, progress: f32) -> Option<WorldLabel<'a>> {
        let (x, y, z) = e.render_pos(progress);
        // Ein SPIELER traegt den Namen aus der Tabellenliste, nicht aus seinen Metadaten.
        // Alles andere daran ist dasselbe wie bei jeder anderen Beschriftung -- und genau
        // deshalb steht es hier und nicht mehr in einem eigenen Zeichenpfad.
        if world::is_player(e.entity_type) {
            let anker = DVec3::new(x, y + self.name_label_height(e), z);
            // Vanillas vier Regeln auf einmal (s. player_label_visible). Die Entfernung misst
            // Vanilla zum ENTITY, nicht zum Ankerpunkt -- daher hier die Fussposition.
            let dist_sq = self.camera.view_position().distance_squared(DVec3::new(x, y, z));
            if !player_label_visible(Some(e.uuid) == self.own_uuid, e.invisible, e.sneaking(), dist_sq) {
                return None;
            }
            let name = self.player_names.get(&e.uuid)?;
            // Vanillas Team-Regel (s. team_allows_name). Der eigene Teamname kommt ueber den
            // eigenen Spielernamen -- Teams fuehren Spieler namentlich, nicht per UUID.
            let team_name = self.sb_entry_team.get(name).map(String::as_str);
            let own_team = self
                .own_uuid
                .and_then(|u| self.player_names.get(&u))
                .and_then(|eigener| self.sb_entry_team.get(eigener))
                .map(String::as_str);
            let team = team_name.and_then(|t| self.sb_teams.get(t));
            if !team_allows_name(team, own_team, team_name) {
                return None;
            }
            // Letzte Zeile von LivingEntityRenderer.hasLabel: `&& !livingEntity.hasPassengers()`.
            // Sonst stehen Namen doppelt da: Server setzen ihr eigenes Namensschild als
            // Text-Display-PASSAGIER auf den Spieler -- Vanilla zeigt dann den Spielernamen
            // nicht. Die Regel steht in Vanilla HINTER dem Team-Switch, und der kehrt fuer jede
            // Sichtbarkeitsregel selbst zurueck; sie greift also nur bei Spielern ohne Team.
            if team.is_none() && self.entities.has_passengers(id) {
                return None;
            }
            // Vanillas Team.decorateName: Prefix + Name + Suffix, alles in der Teamfarbe
            // (PlayerEntity.getDisplayName -> Team.decorateName). Genau daran erkennt man auf
            // den meisten Servern Rang und Fraktion.
            let farbe = team.and_then(|t| protocol::nbt::formatting_color(t.color));
            let beschriftet = match team {
                Some(t) => format!("{}\u{a7}r{name}{}", t.prefix, t.suffix),
                None => name.clone(),
            };
            // Sprachchat: ein Notenzeichen vor dem Namen -- gruen beim Sprechen, tuerkis beim
            // Fluestern, rot fuer taub geschaltete Spieler.
            let beschriftet = match self.voice_nametag_icon(&e.uuid) {
                Some(code) => format!("\u{a7}{code}\u{266a} \u{a7}r{beschriftet}"),
                None => beschriftet,
            };
            return Some(WorldLabel {
                anchor: anker,
                // Prefix/Suffix koennen eigene Paragraphen-Codes tragen -- deshalb durch
                // denselben Auswerter wie jeder andere Text, statt sie roh anzuzeigen.
                spans: std::borrow::Cow::Owned(protocol::nbt::legacy_to_spans(&beschriftet, farbe)),
                world_scale: 1.0,
                background: protocol::packets::TEXT_DISPLAY_INITIAL_BACKGROUND,
                opacity: 0xFF,
                // Wie Vanilla: ein stehender oder laufender Spieler zeigt seinen Namen auch
                // durch Waende (die SEETHROUGH-Pipeline testet keine Tiefe), ein
                // schleichender nur in Sichtlinie.
                see_through: !e.sneaking(),
                shadow: true,
                align: LabelAlign::Center,
                above_anchor: false,
                // Ein Namensschild bricht in Vanilla nicht um.
                line_width: f32::INFINITY,
                // Schleichend halbiert Vanilla die Sichtweite auf 32 Bloecke
                // (LivingEntityRenderer.hasLabel: `if (d >= 1024.0) return false`).
                max_distance: if e.sneaking() { NAMETAG_SNEAK_DISTANCE } else { NAMETAG_MAX_DISTANCE },
                display_matrix: None,
            });
        }
        if let Some(display) = &e.display {
            let world::DisplayContent::Text(text) = &display.content else { return None };
            if text.spans.is_empty() {
                return None;
            }
            return Some(WorldLabel {
                // Die Verschiebung der AffineTransformation gehoert zum Anker; Drehung und
                // Skalierung wirken danach (s. Kommentar an build_world_labels).
                anchor: DVec3::new(x + f64::from(display.translation[0]), y + f64::from(display.translation[1]), z + f64::from(display.translation[2])),
                spans: std::borrow::Cow::Borrowed(&text.spans),
                // Die Y-Skalierung bestimmt die Schrifthoehe; X/Z getrennt zu behandeln
                // hiesse, die Schrift zu verzerren, was der Bitmap-Font nicht kann.
                world_scale: display.scale[1],
                background: if text.flags & protocol::packets::TEXT_DISPLAY_FLAG_DEFAULT_BACKGROUND != 0 {
                    protocol::packets::TEXT_DISPLAY_INITIAL_BACKGROUND
                } else {
                    text.background
                },
                opacity: text.opacity,
                see_through: text.flags & protocol::packets::TEXT_DISPLAY_FLAG_SEE_THROUGH != 0,
                shadow: text.flags & protocol::packets::TEXT_DISPLAY_FLAG_SHADOW != 0,
                align: LabelAlign::from_flags(text.flags),
                above_anchor: true,
                line_width: WORLD_LABEL_DEFAULT_LINE_WIDTH,
                max_distance: f64::from(display.view_range).max(0.0) * DISPLAY_VIEW_RANGE_BLOCKS,
                display_matrix: Some({
                    let (cam_yaw, cam_pitch) = self.vanilla_camera_angles();
                    let rel = (DVec3::new(x, y, z) - self.camera.position).as_vec3();
                    glam::Mat4::from_translation(rel)
                        * display_billboard_matrix(display.billboard, e.render_yaw(progress), e.render_pitch(progress), cam_yaw, cam_pitch)
                        * display_affine_matrix(&display.transform_at(e.age_ticks(progress)))
                }),
            });
        }
        // Namensschild eines Entities mit eigenem Namen. Vanillas Entity.shouldRenderName()
        // ist genau `isCustomNameVisible()` -- der Name allein genuegt nicht, er muss auch
        // eingeblendet sein.
        if !e.name_visible {
            return None;
        }
        let spans = e.custom_name.as_ref()?;
        Some(WorldLabel {
            anchor: DVec3::new(x, y + self.name_label_height(e), z),
            spans: std::borrow::Cow::Borrowed(spans),
            world_scale: 1.0,
            background: protocol::packets::TEXT_DISPLAY_INITIAL_BACKGROUND,
            opacity: 0xFF,
            see_through: false,
            shadow: true,
            align: LabelAlign::Center,
            above_anchor: false,
            // Ein Namensschild bricht in Vanilla nicht um.
            line_width: f32::INFINITY,
            max_distance: NAMETAG_MAX_DISTANCE,
            display_matrix: None,
        })
    }

    // Vanillas Entity.getNameLabelHeight() = getHeight() + 0.5.
    //
    // Der Armorstand ist hier der wichtige Sonderfall, nicht eine Feinheit: er ist der
    // Traeger fast jedes klassischen Hologramms, und seine Hoehe haengt an zwei Flags. Mit
    // der pauschalen Spielerhoehe von hit_box_size schwebte ein Marker-Hologramm 1.8 Bloecke
    // zu hoch -- also sichtbar an der falschen Stelle.
    pub(crate) fn name_label_height(&self, e: &world::Entity) -> f64 {
        let hoehe = match world::entity_type_name(e.entity_type) {
            Some("armor_stand") => e.armor_stand_height(),
            // Fuer alles andere die Trefferbox samt Jungtier und SCALE.
            _ => e.hit_size().1,
        };
        hoehe + NAMETAG_HEIGHT_ABOVE_HEAD
    }

    // Ist die Sichtlinie zwischen zwei Weltpunkten frei von Bloecken? Ersatz fuer den
    // Tiefentest, den es fuer die im Bildschirmraum gezeichneten Namensschilder nicht gibt.
    //
    // Abgetastet statt exakt durchgerastert: physics::raycast_block laeuft ueber ALLE Zellen
    // der umschliessenden Box, was auf 64 Bloecken Entfernung sechsstellig viele Zellen waeren.
    // Hier wird stattdessen in festen Schritten entlang der Strecke geprueft, ob der Punkt in
    // einer Umriss-Box des dortigen Blocks liegt -- dieselbe Formtabelle, gegen die auch
    // gezielt wird (world::outline_boxes), also z.B. kein Fehlalarm durch hohes Gras.
    //
    // Der Preis der Abtastung: eine hauchduenne Wand genau zwischen zwei Abtastpunkten wird
    // uebersehen. Bei SIGHT_SAMPLE_STEP von 0.25 Bloecken betrifft das nur Formen, die duenner
    // als eine Glasscheibe sind -- und ein kurz zu frueh sichtbares Schild ist der harmlosere
    // Fehler gegenueber einem, das nie erscheint.
    pub(crate) fn sicht_frei(&self, von: DVec3, bis: DVec3) -> bool {
        let strecke = bis - von;
        let laenge = strecke.length();
        if laenge <= f64::EPSILON {
            return true;
        }
        let richtung = strecke / laenge;
        let schritte = (laenge / SIGHT_SAMPLE_STEP).ceil() as i32;
        for i in 1..schritte {
            let p = von + richtung * (f64::from(i) * SIGHT_SAMPLE_STEP);
            let (bx, by, bz) = (p.x.floor(), p.y.floor(), p.z.floor());
            let state = self.world.block_at(bx as i32, by as i32, bz as i32);
            if state == world::STATE_AIR {
                continue;
            }
            let lokal = [(p.x - bx) as f32, (p.y - by) as f32, (p.z - bz) as f32];
            for shape in world::outline_boxes(state, &self.block_shapes) {
                let drin = (0..3).all(|a| lokal[a] >= shape.min[a] && lokal[a] <= shape.max[a]);
                if drin {
                    return false;
                }
            }
        }
        true
    }
}
