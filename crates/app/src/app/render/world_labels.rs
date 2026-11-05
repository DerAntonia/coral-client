// Namensschilder, Schildtexte und Display-Text in der Welt.

use super::*;
use crate::{push_font_marker, push_plain_numbers_marker, push_shadow_marker, push_smooth_scale_marker, rect_verts, sign_border_mode, ShadowRun, SignInstance, SignLayout, SIGN_LAYOUT_KEEP};

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
        gpu.occluded_label_texts.borrow_mut().clear();
        gpu.occluded_label_ends.borrow_mut().clear();
        gpu.occluded_label_quads.borrow_mut().clear();
        gpu.occluded_label_glyphs.borrow_mut().clear();
        gpu.label_font.set(self.settings.font_of(ShadowArea::Nametags));
        self.begin_text_area(gpu, texts, ShadowArea::Nametags);
        push_smooth_scale_marker(texts, true);
        self.build_world_labels_in(gpu, rects, texts);
        push_smooth_scale_marker(texts, false);
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
            // zeichnet ihn dann ohne Tiefentest, hier voll deckend im HUD.
            //
            // Alles andere (schleichend) verdeckt die Tiefe der Welt pixelgenau: es kommt in
            // occluded_label_texts mit der NDC-Tiefe des Ankers und wird nach der Welt, vor der
            // Hand gezeichnet -- voll deckend, nicht halb durchsichtig wie in Vanilla. Nur der
            // Shader-Pfad hat dafuer keine Welt-Tiefe und tastet die Sichtlinie ab.
            let tiefe = (!label.see_through && self.label_depth_usable(gpu)).then(|| clip.z / clip.w);
            if !label.see_through && !self.label_depth_usable(gpu) && !self.sicht_frei(eye, label.anchor) {
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
            // Je Abschnitt einmal gemessen, fuer Zeilenbreite und Schreiben (s. zeilen_schreiben).
            let abschnitt_breiten: Vec<Vec<f32>> = zeilen.iter().map(|z| z.iter().map(|s| gpu.measure_text(&s.text, 1.0)).collect()).collect();
            let breiten: Vec<f32> = abschnitt_breiten.iter().map(|z| z.iter().sum()).collect();
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
            // NICHT auf ganze Pixel gerundet: bei stufenloser Groesse traf die Schrift das Raster
            // ohnehin nicht, gerundet sprang das Schild beim Drehen der Kamera aber pixelweise
            // gegen seinen Anker. Ebenso Oberkante und Abschnittsanfaenge.
            let links = sx - block_breite * scale / 2.0;
            // Ein Text-Display waechst NACH OBEN vom Ankerpunkt weg (Vanillas
            // `translate(1 - m/2, -n, 0)` setzt die Unterkante auf den Ursprung); ein
            // Namensschild sitzt dagegen mittig auf seinem Ankerpunkt.
            let block_px = block_hoehe * scale;
            let oben = if label.above_anchor { sy - block_px } else { sy - block_px / 2.0 };

            // Hintergrundflaeche, ein Schriftpixel Luft ringsum (Vanilla: -1 bis m bzw. n).
            let bg = argb_to_rgba(label.background);
            if bg[3] > 0.0 {
                let flaeche = UiRect {
                    rect: Rect {
                        x: links - scale,
                        y: oben - scale,
                        w: (block_breite + 1.0) * scale,
                        h: (block_hoehe + 1.0) * scale,
                    },
                    color: bg,
                    ..UiRect::FLAT
                };
                match tiefe {
                    Some(z) => {
                        let mut ecken = rect_verts(&flaeche, (sw, sh));
                        for v in &mut ecken {
                            v.local[3] = z;
                        }
                        gpu.occluded_label_quads.borrow_mut().extend_from_slice(&ecken);
                    }
                    None => rects.push(flaeche),
                }
            }

            let deckkraft = f32::from(label.opacity) / 255.0;
            let zeilen_schreiben = |ziel: &mut Vec<UiText>| {
                for (i, zeile) in zeilen.iter().enumerate() {
                    // Ausrichtung innerhalb des Blocks (TextDisplayEntity.TextAlignment).
                    let versatz = match label.align {
                        LabelAlign::Left => 0.0,
                        LabelAlign::Right => block_breite - breiten[i],
                        LabelAlign::Center => (block_breite - breiten[i]) / 2.0,
                    };
                    let mut x = links + versatz * scale;
                    let y = oben + i as f32 * line_pitch * scale;
                    for (span, &breite) in zeile.iter().zip(&abschnitt_breiten[i]) {
                        let mut color = span.color.map_or(COLOR_WHITE, rgb_to_rgba);
                        color[3] *= deckkraft;
                        let breite = breite * scale;
                        push_span_shadow(ziel, span.shadow);
                        ziel.push(UiText { text: span.text.clone(), x, y, color, scale });
                        x += breite;
                    }
                }
            };
            match tiefe {
                // Je Schild einzeln geformt (s. GpuState::occluded_label_texts), darum mit den
                // Markern des Bereichs Nametags am Anfang und ihrem Ende danach.
                Some(z) => {
                    let mut verdeckbar = gpu.occluded_label_texts.borrow_mut();
                    push_shadow_marker(&mut verdeckbar, self.settings.shadow_marker_of(ShadowArea::Nametags));
                    push_font_marker(&mut verdeckbar, self.settings.font_of(ShadowArea::Nametags));
                    push_plain_numbers_marker(&mut verdeckbar, ShadowArea::Nametags.plain_numbers());
                    push_smooth_scale_marker(&mut verdeckbar, true);
                    zeilen_schreiben(&mut verdeckbar);
                    push_smooth_scale_marker(&mut verdeckbar, false);
                    push_shadow_marker(&mut verdeckbar, None);
                    push_font_marker(&mut verdeckbar, None);
                    push_plain_numbers_marker(&mut verdeckbar, false);
                    gpu.occluded_label_ends.borrow_mut().push((verdeckbar.len(), z));
                }
                None => zeilen_schreiben(texts),
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
        self.sign_layouts.borrow_mut().sichtbar.clear();
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
            if !self.block_column_drawn(pos) || (mitte - auge).length_squared() > SIGN_TEXT_DISTANCE * SIGN_TEXT_DISTANCE || self.block_entity_culled(pos) {
                continue;
            }
            for (seite, vorn) in [(&text.front, true), (&text.back, false)] {
                if seite.lines.iter().all(|z| z.iter().all(|s| s.text.is_empty())) || editiert.is_some_and(|e| e.pos == pos && e.front == vorn) {
                    continue;
                }
                let Some((frame, zeile_h)) = self.sign_side_frame(pos, vorn) else { continue };
                // Verdeckung macht der Tiefentest -- nur ohne ihn (Shader-Pfad) der Strahl.
                if self.sign_side_visible(frame, view_proj, zeile_h, (sw, sh), crate::shader_path_on()) {
                    self.push_sign_side(gpu, (pos, vorn), seite, frame, view_proj, zeile_h, (sw, sh));
                }
            }
        }
        if let Some(edit) = editiert {
            self.push_sign_edit_in_world(gpu, edit, view_proj, (sw, sh));
        }
        let mut gesetzt = self.sign_layouts.borrow_mut();
        let gesetzt = &mut *gesetzt;
        let runde = gesetzt.runde;
        let frei = &mut gesetzt.freie_slots;
        gesetzt.seiten.retain(|_, l| {
            let bleibt = l.runde + SIGN_LAYOUT_KEEP > runde;
            if !bleibt {
                frei.push(l.slot);
            }
            bleibt
        });
        gesetzt.runde += 1;
        gpu.upload_signs(gesetzt);
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
        self.push_sign_side(gpu, (edit.pos, edit.front), &seite, frame, view_proj, zeile_h, screen);

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

    // Eine Schildseite fuer den Welt-Durchgang (s. SignLayout, GpuState::upload_signs). Rand:
    // wie bei der Weltschrift, ausser der Bereich Schilder hat ihn ausgeschaltet. Gesetzt wird
    // nur, wenn sich Text, Schrift, Rasterstufe, Rand oder Zeilenhoehe geaendert haben; sonst
    // kommt nur die Matrix der Seite fuer dieses Bild dazu.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_sign_side(
        &self,
        gpu: &GpuState,
        schluessel: ((i32, i32, i32), bool),
        seite: &protocol::sign::SignSide,
        frame: glam::Mat4,
        view_proj: glam::Mat4,
        zeile_h: f32,
        screen: (f32, f32),
    ) {
        let clip = view_proj * frame;
        let project = sign_projector(clip, screen);
        let raster = match (project(0.0, 0.0), project(1.0, 0.0)) {
            (Some(p), Some(q)) => ((q.0 - p.0).hypot(q.1 - p.1)).clamp(1.0, WORLD_LABEL_MAX_SCALE),
            _ => 1.0,
        };
        let atlas = gpu.layout_atlas();
        let generation = font_atlas::glyph_generation();
        let stufe = atlas.linear_raster(raster).ok_or(raster.to_bits());
        let lauf = self.settings.shadow_marker_of(ShadowArea::Signs);
        let lauf_wert = lauf.unwrap_or((gpu.text_shadow_style, gpu.text_shadow_strength, gpu.text_shadow_half));
        let mut gesetzt = self.sign_layouts.borrow_mut();
        let gesetzt = &mut *gesetzt;
        let runde = gesetzt.runde;
        let gilt = gesetzt.seiten.get(&schluessel).is_some_and(|l| l.gilt(seite, generation, atlas.kind(), stufe, lauf_wert, zeile_h));
        if !gilt {
            let slot = match gesetzt.seiten.get(&schluessel) {
                Some(alt) => alt.slot,
                None => gesetzt.freie_slots.pop().unwrap_or_else(|| {
                    gesetzt.slots += 1;
                    gesetzt.slots - 1
                }),
            };
            let instanzen = layout_sign_side(gpu, seite, zeile_h, raster, lauf, slot);
            let neu = SignLayout { seite: seite.clone(), generation, font: atlas.kind(), raster: stufe, lauf: lauf_wert, zeile_h, instanzen, slot, gpu: None, runde };
            gesetzt.seiten.insert(schluessel, neu);
        }
        if let Some(l) = gesetzt.seiten.get_mut(&schluessel) {
            l.runde = runde;
        }
        gesetzt.sichtbar.push((schluessel, clip));
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
        // Verdeckung wie bei den Namensschildern: ohne Durchscheinen gegen die Tiefe der Welt,
        // hier mit der Tiefe jeder Ecke (die Flaeche steht schraeg im Raum). Nur der
        // Shader-Pfad tastet die Sichtlinie zur Mitte ab.
        let verdeckbar = !label.see_through && self.label_depth_usable(gpu);
        if !label.see_through && !verdeckbar {
            let mitte = frame * glam::Vec4::new((m - 1.0) / 2.0, (n - 1.0) / 2.0, 0.0, 1.0);
            let welt = self.camera.position + glam::DVec3::new(f64::from(mitte.x), f64::from(mitte.y), f64::from(mitte.z));
            if !self.sicht_frei(self.camera.view_position(), welt) {
                return;
            }
        }
        let tiefe = |x: f32, y: f32| {
            let c = clip * glam::Vec4::new(x, y, 0.0, 1.0);
            c.z / c.w
        };

        let bg = argb_to_rgba(label.background);
        if bg[3] > 0.0 {
            let v = |p: (f32, f32), z: f32| {
                let mut v = Vertex { position: pixel_to_ndc(p.0, p.1, screen), color: bg, ..Vertex::PLAIN };
                v.local[3] = z;
                v
            };
            let z = [tiefe(-1.0, -1.0), tiefe(m, -1.0), tiefe(m, n), tiefe(-1.0, n)];
            let ecken = [v(a, z[0]), v(b, z[1]), v(c, z[2]), v(a, z[0]), v(c, z[2]), v(d, z[3])];
            if verdeckbar {
                gpu.occluded_label_quads.borrow_mut().extend_from_slice(&ecken);
            } else {
                gpu.world_label_quads.borrow_mut().extend_from_slice(&ecken);
            }
        }

        // Rasterstufe der Glyphen: wie viele Bildschirmpixel ein Schriftpixel etwa breit ist.
        let raster = match (project(m / 2.0, n / 2.0), project(m / 2.0 + 1.0, n / 2.0)) {
            (Some(p), Some(q)) => ((q.0 - p.0).hypot(q.1 - p.1)).clamp(1.0, WORLD_LABEL_MAX_SCALE),
            _ => 1.0,
        };
        let deckkraft = f32::from(label.opacity) / 255.0;
        let mut glyphen = if verdeckbar { gpu.occluded_label_glyphs.borrow_mut() } else { gpu.world_label_glyphs.borrow_mut() };
        let tiefe_je_punkt: Option<&dyn Fn(f32, f32) -> f32> = verdeckbar.then_some(&tiefe);
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
                let rand = gpu.world_text_border(color, span.shadow, label.shadow, None);
                x += gpu.shape_projected_text(&span.text, x, top, color, rand, raster, &project, tiefe_je_punkt, screen, &mut glyphen);
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
            // Wie bei Spielern: EntityRenderer.submitNameTag zeichnet JEDES Namensschild mit
            // seeThrough = !isDiscrete, also auch Hologramme und benannte Mobs durch Waende.
            see_through: !e.sneaking(),
            shadow: true,
            align: LabelAlign::Center,
            above_anchor: false,
            // Ein Namensschild bricht in Vanilla nicht um.
            line_width: f32::INFINITY,
            // Schleichend 32 Bloecke wie bei Spielern (LivingEntityRenderer.shouldShowName).
            max_distance: if e.sneaking() { NAMETAG_SNEAK_DISTANCE } else { NAMETAG_MAX_DISTANCE },
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

    // Taugt die Tiefe der Welt fuer die verdeckbaren Weltbeschriftungen? Nicht im Shader-Pfad
    // (eigene Tiefe, kein Durchgang dafuer) und nicht mit Warp-Projektion: die verbiegt die Welt
    // im Vertex-Shader, die Tiefe aus view_proj passt dann nicht. Dort bleibt die Sichtlinie.
    pub(crate) fn label_depth_usable(&self, _gpu: &GpuState) -> bool {
        !crate::shader_path_on() && self.camera.projection == 0
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

// Setzt eine Schildseite in Schriftpixeln ihrer Textebene (s. SignLayout): Zeile i bei
// y = i * h - 2 * h, mittig um x = 0 nach der Breite ihrer Glyphen, je Farbabschnitt erst die
// Raender, dann die Glyphen. `raster` = Bildschirmpixel je Schriftpixel, waehlt die
// Rasterstufe (s. GpuState::shape_projected_text); `slot` = Platz der Matrix der Seite.
fn layout_sign_side(gpu: &GpuState, seite: &protocol::sign::SignSide, zeile_h: f32, raster: f32, lauf: Option<ShadowRun>, slot: u32) -> Vec<SignInstance> {
    let atlas = gpu.layout_atlas();
    let vorgabe = sign_text_color(seite.color, seite.glowing);
    let mut instanzen = Vec::new();
    // Die Glyphen eines Abschnitts: Rechteck, UV, Pack.
    let mut glyphen: Vec<([f32; 4], [f32; 4], bool)> = Vec::new();
    for (i, zeile) in seite.lines.iter().enumerate() {
        let grundlinie = i as f32 * zeile_h - 2.0 * zeile_h + atlas.ascent_px();
        let anfang = instanzen.len();
        let mut pen = 0.0;
        for span in zeile {
            let color = rgb_to_rgba(span.color.unwrap_or(vorgabe));
            glyphen.clear();
            for ch in atlas.display_text(&span.text).chars() {
                let Some(g) = atlas.glyph(ch) else { continue };
                if let Some(p) = atlas.place(ch, raster).filter(|p| p.size[0] > 0.0 && p.size[1] > 0.0) {
                    let x0 = pen + p.offset[0] / raster;
                    let y0 = grundlinie + p.offset[1] / raster;
                    glyphen.push(([x0, y0, x0 + p.size[0] / raster, y0 + p.size[1] / raster], [p.uv_min[0], p.uv_min[1], p.uv_max[0], p.uv_max[1]], p.pack));
                }
                pen += g.advance_px;
            }
            let pack = |art: f32, pack: bool| if pack { art + crate::text_draw::TEXT_MODE_PACK } else { art };
            let rand = gpu.world_text_border(color, span.shadow, true, lauf).and_then(|((stil, staerke, halb), farbe)| sign_border_mode(stil, staerke, halb, farbe));
            if let Some((art, wert, farbe)) = rand {
                instanzen.extend(glyphen.iter().map(|&(rect, uv, p)| SignInstance { rect, uv, color: farbe, mode: [pack(art, p), wert, 0.0, 0.0], seite: slot }));
            }
            instanzen.extend(glyphen.iter().map(|&(rect, uv, p)| SignInstance { rect, uv, color, mode: [pack(0.0, p), 0.0, 0.0, 0.0], seite: slot }));
        }
        let halb = pen / 2.0;
        for q in &mut instanzen[anfang..] {
            q.rect[0] -= halb;
            q.rect[2] -= halb;
        }
    }
    instanzen
}
