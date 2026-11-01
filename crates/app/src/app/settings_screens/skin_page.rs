// Skin-Seite, Skin-Dateien und Server-Pakete.

use super::*;

impl App {
    // Skin-Tab: zwei Spalten mit Kacheln (s. skin_tab). Links, was im Skin-Ordner liegt und
    // nicht gewaehlt ist, rechts der Stapel -- das obere Pack ueberschreibt die darunter,
    // ganz unten steht fest Vanilla. Kachel: Bild links, Name oben, Beschreibung darunter.
    pub(crate) fn build_settings_skin_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        use skin_tab::Column;
        use theme::Slot;
        let g = theme::get;
        let (px, py) = self.pointer();
        let frei = self.skin_drag_active().is_none() && self.skin_files.is_none();
        for (c, titel) in [(Column::Available, "Verfügbar"), (Column::Selected, "Ausgewählt")] {
            let col = skin_column_rect(c);
            let y = (col.y + (SKIN_HEAD_H - 8.0 - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: titel.to_string(), x: col.x, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        if self.skin_catalog.names.is_empty() {
            let col = skin_column_rect(Column::Available);
            for (i, zeile) in wrap_text(gpu, "Noch keine Packs. Rechts auf \"Ordner öffnen\" und ein Pack hineinlegen, als Ordner oder Zip.", UI_SCALE_SMALL, col.w).into_iter().enumerate() {
                texts.push(UiText { text: zeile, x: col.x, y: skin_tiles_top() + 8.0 + i as f32 * (gpu.text_height(UI_SCALE_SMALL) + 4.0), color: theme::text(), scale: UI_SCALE_SMALL });
            }
        }
        let klein = gpu.text_height(UI_SCALE_SMALL);
        // Beim gleitenden Scrollen ragen die Kacheln am Rand nur halb herein: Flaechen und Bild
        // werden abgeschnitten, Text steht nur, wo er ganz hineinpasst (wie in der Serverliste).
        let (oben, unten) = (skin_tiles_top(), skin_tiles_bottom());
        let passt = |y: f32, h: f32| y >= oben && y + h <= unten;
        // Jede Spalte scrollt fuer sich, also auch ihr Verlauf.
        let fades = [Column::Available, Column::Selected].map(|c| {
            let stand = self.skin_scroll[usize::from(c == Column::Selected)].stand;
            ScrollFade::new(oben, unten, skin_tile_h(), stand, self.skin_max_scroll(c))
        });
        for (spalte, _, name, karte) in self.skin_tiles() {
            let fade = fades[usize::from(spalte == Column::Selected)];
            push_fade_marker(rects, texts, Some(fade));
            let Some(sichtbar) = skin_clip(karte) else { continue };
            let hover = frei && sichtbar.contains(px, py);
            let (fill, rand) = if hover { (g(Slot::Surface2), g(Slot::Accent)) } else { (g(Slot::Surface1), g(Slot::Border1)) };
            rects.push(UiRect::shadow(sichtbar, 8.0, 2.0, 8.0, 0.35));
            rects.push(UiRect::framed(sichtbar, fill, 8.0, 1.0, rand));
            let bild = skin_icon_rect(karte);
            if let Some(b) = skin_clip(bild) {
                rects.push(UiRect::rounded(b, g(Slot::SurfaceSunken), 6.0));
                if let Some(platz) = self.skin_icon_slot(&name) {
                    let [u0, v0, u1, v1] = pack_icon_uv(platz);
                    let (t0, t1) = ((b.y - bild.y) / bild.h, (b.y + b.h - bild.y) / bild.h);
                    let uv = [u0, v0 + (v1 - v0) * t0, u1, v0 + (v1 - v0) * t1];
                    fade_sprite(UiSprite::quad(b.x, b.y, b.w, b.h, uv, [1.0; 4], SpriteAtlas::PackIcons), fade, sprites);
                }
            }
            // Die Zahnraeder auf dem Bild zeichnet build_skin_gears darueber.
            let tx = bild.x + bild.w + 16.0;
            let breite = (karte.x + karte.w - 16.0 - tx).max(40.0);
            let titel = match name.as_str() {
                skin_tab::VANILLA => "Vanilla",
                skin_tab::SERVER => "Server-Pack",
                n => n,
            };
            let zeile1 = (karte.y + 14.0).round();
            if passt(zeile1, gpu.text_height(UI_SCALE_NORMAL)) {
                texts.push(UiText { text: ellipsize(gpu, titel, UI_SCALE_NORMAL, breite), x: tx, y: zeile1, color: theme::text(), scale: UI_SCALE_NORMAL });
            }
            let fest = |t: &str| vec![protocol::nbt::TextSpan { text: t.to_string(), ..Default::default() }];
            let beschreibung: Vec<protocol::nbt::TextSpan> = match name.as_str() {
                skin_tab::VANILLA => fest("Die Standard-Texturen von Minecraft. Füllt auf, was die Packs darüber nicht mitbringen; Packs darunter ergänzen nur noch, was Vanilla nicht hat."),
                skin_tab::SERVER => fest("Vom Server geschickt. Steht anfangs ganz oben; weiter unten überschreibt es nur noch, was die Packs darüber nicht haben. Platz und Zahnrad merkt sich der Client für diesen Server und dieses Pack."),
                n => self.skin_catalog.get(n).map(|e| e.description.clone()).unwrap_or_default(),
            };
            let zeilen: Vec<Vec<protocol::nbt::TextSpan>> =
                wrap_spans(&beschreibung, breite, |t| gpu.measure_text(t, UI_SCALE_SMALL), 0.0).into_iter().map(|l| l.spans).filter(|z| !z.is_empty()).collect();
            let y0 = zeile1 + gpu.text_height(UI_SCALE_NORMAL) + 10.0;
            let passen = (((karte.y + karte.h - 12.0 - y0 + 4.0) / (klein + 4.0)).floor() as usize).max(1);
            for (k, z) in zeilen.iter().take(passen).enumerate() {
                let y = (y0 + k as f32 * (klein + 4.0)).round();
                if !passt(y, klein) {
                    continue;
                }
                let mut z = z.clone();
                // Mehr Text als Platz: die letzte Zeile endet mit "...".
                if k + 1 == passen && zeilen.len() > passen {
                    z.push(protocol::nbt::TextSpan { text: " ".to_string(), ..Default::default() });
                    z.extend(zeilen[passen].iter().cloned());
                }
                let z = ellipsize_spans(gpu, &z, UI_SCALE_SMALL, breite);
                push_styled_row(gpu, &z, tx, y, theme::text_2(), UI_SCALE_SMALL, texts, &mut |r, c| rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT }));
            }
        }
        push_fade_marker(rects, texts, None);
        // Gezogene Kachel: Linie in der Luecke, in der sie landen wuerde.
        if let Some((c, platz)) = self.skin_drop_slot() {
            let stand = self.skin_scroll[usize::from(c == Column::Selected)].stand;
            let col = skin_column_rect(c);
            let y = (skin_tile_rect(c, platz as f32 - stand).y - SKIN_TILE_GAP / 2.0 - 1.0).round();
            if y >= oben - SKIN_TILE_GAP && y <= unten {
                rects.push(UiRect::rounded(Rect { x: col.x, y, w: col.w, h: 2.0 }, theme::text(), 1.0));
            }
        }
        // Bildlaufleiste je Spalte, sobald nicht alle Kacheln passen.
        for (k, c) in [Column::Available, Column::Selected].into_iter().enumerate() {
            let n = self.skin_column_len(c);
            let sichtbar = skin_tiles_visible();
            if n <= sichtbar {
                continue;
            }
            let col = skin_column_rect(c);
            let spur = Rect { x: col.x + col.w + 8.0, y: oben, w: 4.0, h: unten - oben };
            rects.push(UiRect::rounded(spur, g(Slot::Surface1), 2.0));
            let griff_h = (spur.h * sichtbar as f32 / n as f32).max(32.0);
            let max = n - sichtbar;
            let griff = Rect { y: spur.y + (spur.h - griff_h) * (self.skin_scroll[k].stand / max as f32).clamp(0.0, 1.0), h: griff_h, ..spur };
            rects.push(UiRect::rounded(griff, g(Slot::Border2), 2.0));
        }
    }

    // Die beiden Spalten: links verfuegbar (s. skin_tab::available_column), rechts der Stapel.
    pub(crate) fn skin_columns(&self) -> (Vec<String>, Vec<String>) {
        (skin_tab::available_column(&self.skin_catalog.names, &self.settings.skin_order, &self.selected_skins), self.selected_skins.clone())
    }

    // So weit laesst sich eine Spalte scrollen, in Kacheln.
    pub(crate) fn skin_max_scroll(&self, c: skin_tab::Column) -> f32 {
        self.skin_column_len(c).saturating_sub(skin_tiles_visible()) as f32
    }

    // Die sichtbaren Kacheln in Inhaltskoordinaten: Spalte, Index in der Spalte, Name (None =
    // die feste Vanilla-Kachel unter dem Stapel) und Rechteck. Waehrend eine Kachel gezogen
    // wird, fehlt sie, und die uebrigen ruecken zusammen.
    pub(crate) fn skin_tiles(&self) -> Vec<(skin_tab::Column, usize, String, Rect)> {
        use skin_tab::Column;
        let (links, rechts) = self.skin_columns();
        let gezogen = self.skin_drag_active().map(|d| (d.column, d.index));
        let mut out = Vec::new();
        for (k, (c, liste)) in [(Column::Available, links), (Column::Selected, rechts)].into_iter().enumerate() {
            let reihe = liste.into_iter().enumerate().filter(|(i, _)| gezogen != Some((c, *i)));
            let stand = self.skin_scroll[k].stand;
            for (platz, (i, name)) in reihe.enumerate() {
                let r = skin_tile_rect(c, platz as f32 - stand);
                if skin_clip(r).is_some() {
                    out.push((c, i, name, r));
                }
            }
        }
        out
    }

    // Kacheln einer Spalte, ohne die gerade gezogene.
    pub(crate) fn skin_column_len(&self, c: skin_tab::Column) -> usize {
        let (links, rechts) = self.skin_columns();
        let n = if c == skin_tab::Column::Available { links.len() } else { rechts.len() };
        n - usize::from(self.skin_drag_active().is_some_and(|d| d.column == c))
    }

    // Bildplatz im Pack-Bilder-Atlas: Vanilla hat einen festen, ein Server-Pack keinen.
    pub(crate) fn skin_icon_slot(&self, name: &str) -> Option<usize> {
        match name {
            skin_tab::VANILLA => self.skin_catalog.vanilla_icon_done.then_some(skin_tab::VANILLA_SLOT),
            skin_tab::SERVER => None,
            _ => self.skin_catalog.get(name).and_then(|e| e.icon_slot),
        }
    }

    // Die gezogene Kachel -- erst ab SERVER_DRAG_START_PX Bewegung, danach bis zum Loslassen.
    pub(crate) fn skin_drag_active(&self) -> Option<&SkinDrag> {
        let d = self.skin_drag.as_ref()?;
        if !d.moved.get() {
            let (x, y) = self.cursor_pos;
            if (x - d.press.0).abs() + (y - d.press.1).abs() > SERVER_DRAG_START_PX {
                d.moved.set(true);
            }
        }
        d.moved.get().then_some(d)
    }

    // Wohin die gezogene Kachel fiele: Spalte und Platz unter den dortigen Packs ohne sie,
    // die Luecke dem Zeiger am naechsten. None neben den Spalten (Loslassen dort bricht ab).
    pub(crate) fn skin_drop_slot(&self) -> Option<(skin_tab::Column, usize)> {
        use skin_tab::Column;
        let d = self.skin_drag_active()?;
        let (x, y) = self.pointer();
        let c = [Column::Available, Column::Selected].into_iter().find(|&c| skin_column_rect(c).contains(x, y))?;
        // Vanilla und das Server-Pack gibt es nur im Stapel.
        let (links, rechts) = self.skin_columns();
        let gezogen = if d.column == Column::Available { links.get(d.index) } else { rechts.get(d.index) };
        if c == Column::Available && gezogen.is_some_and(|n| skin_tab::is_marker(n)) {
            return None;
        }
        let n = self.skin_column_len(c);
        let stand = self.skin_scroll[usize::from(c == Column::Selected)].stand;
        let schritt = skin_tile_h() + SKIN_TILE_GAP;
        let platz = ((y - skin_tiles_top() + SKIN_TILE_GAP / 2.0) / schritt + stand + 0.5).floor().max(0.0) as usize;
        Some((c, platz.min(n)))
    }

    // Hover-Ziel im Skin-Tab (s. HoverTarget). None = nicht zustaendig, die Knoepfe gelten;
    // Some(None) = hier klingt nichts (offenes Dateifenster daneben, Ziehen).
    pub(crate) fn skin_hover_target(&self) -> Option<Option<HoverTarget>> {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsSkin {
            return None;
        }
        if let Some(w) = &self.skin_files {
            let (x, y) = self.cursor_pos;
            let (p, _, alles, fertig) = skin_files_layout();
            let ziel = if fertig.contains(x, y) {
                Some(HoverTarget::SkinFilesDone)
            } else if alles.contains(x, y) {
                Some(HoverTarget::SkinFilesAll)
            } else if !p.contains(x, y) {
                None
            } else {
                self.skin_files_rows().into_iter().find(|(_, r)| r.contains(x, y)).map(|(i, zeile)| {
                    let e = &w.entries[i];
                    let kasten = Self::skin_files_check_rect(zeile, e.depth);
                    HoverTarget::SkinFileRow(i, !e.is_dir || (kasten.x - 4.0..=kasten.x + kasten.w + 4.0).contains(&x))
                })
            };
            return Some(ziel);
        }
        if self.skin_drag_active().is_some() {
            return Some(None);
        }
        let (x, y) = self.pointer();
        let (c, i, name, r) = self.skin_tiles().into_iter().find(|t| skin_clip(t.3).is_some_and(|r| r.contains(x, y)))?;
        let bild = self.skin_has_settings(&name) && skin_icon_rect(r).contains(x, y);
        Some(Some(HoverTarget::SkinTile(c, i, bild)))
    }

    // Linke Taste auf einer Kachel: nur vormerken. Ob es ein Klick oder ein Ziehen wird,
    // entscheidet die Bewegung bis zum Loslassen (s. skin_drag_release).
    pub(crate) fn skin_tile_press(&mut self) -> bool {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsSkin {
            return false;
        }
        let (x, y) = self.pointer();
        let Some((column, index, _, _)) = self.skin_tiles().into_iter().find(|t| skin_clip(t.3).is_some_and(|r| r.contains(x, y))) else { return false };
        self.skin_drag = Some(SkinDrag { column, index, press: self.cursor_pos, moved: std::cell::Cell::new(false) });
        true
    }

    pub(crate) fn skin_drag_release(&mut self) {
        use skin_tab::Column;
        let gezogen = self.skin_drag_active().is_some();
        let ziel = self.skin_drop_slot();
        let Some(d) = self.skin_drag.take() else { return };
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsSkin {
            return;
        }
        let (mut links, mut rechts) = self.skin_columns();
        let ziel = if gezogen {
            let Some(ziel) = ziel else { return };
            ziel
        } else {
            // Klick aufs Bild: Einstellungen des Packs. Sonst in die andere Spalte, dort oben.
            let (x, y) = self.pointer();
            let kachel = self.skin_tiles().into_iter().find(|t| t.0 == d.column && t.1 == d.index);
            let auf_bild = kachel.as_ref().filter(|t| self.skin_has_settings(&t.2) && skin_icon_rect(t.3).contains(x, y)).map(|t| t.2.clone());
            if let Some(name) = auf_bild {
                self.play_ui_sound(ui_sound::UiSound::Open);
                if name == skin_tab::SERVER {
                    self.open_server_pack_files();
                } else {
                    self.open_skin_files(name);
                }
                return;
            }
            // Vanilla und das Server-Pack wechseln nicht die Spalte.
            if kachel.as_ref().is_some_and(|t| skin_tab::is_marker(&t.2)) {
                return;
            }
            (if d.column == Column::Available { Column::Selected } else { Column::Available }, 0)
        };
        skin_tab::move_skin(&mut links, &mut rechts, (d.column, d.index), ziel);
        self.selected_skins = rechts;
        self.settings.skin_order = links;
        self.remember_server_pack_position();
        self.play_ui_sound(ui_sound::UiSound::Click);
        self.persist_settings();
    }

    // Was in diesen Packs ausgeschaltet ist (s. Settings::skin_exclusions).
    // Fuer den Server-Eintrag stehen darin die Server-Packs (s. server_pack_exclusions).
    pub(crate) fn skin_exclusions_of(&self, skins: &[String]) -> std::collections::BTreeMap<String, Vec<String>> {
        let mut out: std::collections::BTreeMap<String, Vec<String>> =
            self.settings.skin_exclusions.iter().filter(|(k, _)| skins.contains(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
        if skins.iter().any(|n| n == skin_tab::SERVER) {
            for (id, p) in self.server_packs.ready() {
                let (hash, adresse) = server_packs::keys(self.server_packs.address(), id, &p);
                out.insert(hash.unwrap_or(adresse), self.server_pack_exclusions(id, &p));
            }
        }
        out
    }

    // Beim Verlassen des Skin-Tabs: den gewaehlten Stapel laden, wenn er (oder was in seinen
    // Packs ausgeschaltet ist) vom geladenen abweicht.
    pub(crate) fn apply_skin_tab(&mut self) {
        self.skin_drag = None;
        self.skin_files = None;
        let geaendert = self.selected_skins != self.settings.active_skins || self.skin_exclusions_of(&self.selected_skins) != self.loaded_skin_exclusions;
        if geaendert {
            self.reload_resource_pack(self.selected_skins.clone());
        }
    }

    // Einmal je Bild: solange der Skin-Tab offen ist, den Ordner verfolgen und neue Pack-Bilder
    // hochladen; beim Verlassen laden.
    pub(crate) fn tick_skin_tab(&mut self) {
        let offen = self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsSkin;
        if offen {
            if !self.skin_tab_was_open {
                self.skin_catalog.invalidate();
                for (i, c) in [skin_tab::Column::Available, skin_tab::Column::Selected].into_iter().enumerate() {
                    let max = self.skin_max_scroll(c);
                    let s = &mut self.skin_scroll[i];
                    s.jump(s.target(max));
                }
            }
            let neu = self.skin_catalog.refresh(skins_dir().as_deref(), list_available_skins);
            if !self.skin_catalog.vanilla_icon_done {
                self.skin_catalog.vanilla_icon_done = true;
                if let Some(rgba) = self.installation.vanilla_pack_icon().and_then(|b| skin_tab::decode_icon(&b)) {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.upload_pack_icon(skin_tab::VANILLA_SLOT, &rgba);
                    }
                }
            }
            if let Some(gpu) = self.gpu.as_mut() {
                for (platz, rgba) in neu {
                    gpu.upload_pack_icon(platz, &rgba);
                }
            }
        } else if self.skin_tab_was_open {
            self.apply_skin_tab();
        }
        self.skin_tab_was_open = offen;
    }

    pub(crate) fn open_skin_files(&mut self, skin: String) {
        let Some(dir) = skins_dir() else { return };
        let entries = skin_tab::list_tree(&dir.join(&skin).join("assets").join("minecraft"));
        self.skin_files = Some(SkinFilesWindow { title: skin.clone(), skin, mirror: None, entries, expanded: std::collections::HashSet::new(), scroll: Default::default() });
    }

    // Dateifenster fuer das oberste Server-Pack (das Zip im Cache).
    pub(crate) fn open_server_pack_files(&mut self) {
        let packs = self.server_packs.ready();
        let Some((id, pfad)) = packs.first().cloned() else { return };
        let (hash, adresse) = server_packs::keys(self.server_packs.address(), id, &pfad);
        let namen = assets::zip_file_names(&pfad);
        let entries = skin_tab::tree_from_paths(namen.iter().filter_map(|n| n.strip_prefix("assets/minecraft/")));
        let (skin, mirror) = match hash {
            Some(h) => (h, Some(adresse)),
            None => (adresse, None),
        };
        // Bisher nur nach Adresse gespeichert (der Server hat sein Pack geaendert): fuer dieses
        // Pack uebernehmen, so wie es beim Laden schon gilt.
        if let Some(m) = &mirror {
            if !self.settings.skin_exclusions.contains_key(&skin) {
                if let Some(v) = self.settings.skin_exclusions.get(m).cloned() {
                    self.settings.skin_exclusions.insert(skin.clone(), v);
                }
            }
        }
        let title = if packs.len() > 1 { format!("Server-Pack (oberstes von {})", packs.len()) } else { "Server-Pack".to_string() };
        self.skin_files = Some(SkinFilesWindow { skin, mirror, title, entries, expanded: std::collections::HashSet::new(), scroll: Default::default() });
    }

    // Nach jeder Aenderung im Dateifenster: bei einem Server-Pack dasselbe auch unter der
    // Adresse speichern.
    pub(crate) fn sync_skin_files_mirror(&mut self) {
        let Some((skin, Some(m))) = self.skin_files.as_ref().map(|w| (w.skin.clone(), w.mirror.clone())) else { return };
        match self.settings.skin_exclusions.get(&skin).cloned() {
            Some(v) => self.settings.skin_exclusions.insert(m, v),
            None => self.settings.skin_exclusions.remove(&m),
        };
    }

    // Was in einem Server-Pack ausgeschaltet ist: nach seinem SHA-1 gespeichert, sonst nach
    // Server-Adresse und Pack-UUID (s. server_packs::keys).
    pub(crate) fn server_pack_exclusions(&self, id: [u8; 16], path: &std::path::Path) -> Vec<String> {
        let (hash, adresse) = server_packs::keys(self.server_packs.address(), id, path);
        let aus = &self.settings.skin_exclusions;
        hash.and_then(|h| aus.get(&h)).or_else(|| aus.get(&adresse)).cloned().unwrap_or_default()
    }

    // Gemerkter Platz des Server-Eintrags: nach Server-Adresse, sonst nach dem SHA-1 eines der
    // Packs, sonst ganz oben.
    pub(crate) fn server_pack_position(&self, packs: &[([u8; 16], std::path::PathBuf)]) -> usize {
        let plaetze = &self.settings.server_pack_positions;
        let adresse = self.server_packs.address();
        plaetze
            .get(&server_packs::position_key(adresse))
            .or_else(|| packs.iter().find_map(|(id, p)| server_packs::keys(adresse, *id, p).0.and_then(|h| plaetze.get(&h))))
            .copied()
            .unwrap_or(0)
    }

    // Nach dem Verschieben im Skin-Tab: wo der Server-Eintrag jetzt steht, unter der Adresse und
    // dem SHA-1 jedes Server-Packs merken.
    pub(crate) fn remember_server_pack_position(&mut self) {
        let Some(platz) = self.selected_skins.iter().position(|n| n == skin_tab::SERVER) else { return };
        let adresse = self.server_packs.address().to_string();
        let mut schluessel = vec![server_packs::position_key(&adresse)];
        schluessel.extend(self.server_packs.ready().iter().filter_map(|(id, p)| server_packs::keys(&adresse, *id, p).0));
        for k in schluessel {
            self.settings.server_pack_positions.insert(k, platz);
        }
    }

    // Hat diese Kachel ein Zahnrad? Vanilla nie, der Server-Eintrag, sobald ein Pack da ist.
    pub(crate) fn skin_has_settings(&self, name: &str) -> bool {
        match name {
            skin_tab::VANILLA => false,
            skin_tab::SERVER => !self.server_packs.ready().is_empty(),
            _ => true,
        }
    }

    // Ist in diesem Pack etwas ausgeschaltet (kleines Zahnrad oben rechts)?
    pub(crate) fn skin_has_exclusions(&self, name: &str) -> bool {
        if name == skin_tab::SERVER {
            return self.server_packs.ready().iter().any(|(id, p)| !self.server_pack_exclusions(*id, p).is_empty());
        }
        self.settings.skin_exclusions.get(name).is_some_and(|v| !v.is_empty())
    }

    // Sichtbare Zeilen des Dateifensters: Index in entries und Rechteck (Bildschirm).
    pub(crate) fn skin_files_rows(&self) -> Vec<(usize, Rect)> {
        let Some(w) = &self.skin_files else { return Vec::new() };
        let (_, liste, _, _) = skin_files_layout();
        let zeilen = skin_tab::visible_rows(&w.entries, &w.expanded);
        let stand = w.scroll.stand;
        // Gleitend; nur ganz sichtbare Zeilen, was ueber den Rand gleitet, verschwindet.
        zeilen
            .into_iter()
            .enumerate()
            .skip(w.scroll.first())
            .take(skin_files_rows_visible() + 1)
            .map(|(k, i)| (i, Rect { x: liste.x, y: liste.y + (k as f32 - stand) * SKIN_FILES_ROW_H, w: liste.w, h: SKIN_FILES_ROW_H }))
            .filter(|(_, r)| r.y >= liste.y - 0.5 && r.y + r.h <= liste.y + liste.h + 0.5)
            .collect()
    }

    // So weit laesst sich das offene Dateifenster scrollen, in Zeilen.
    pub(crate) fn skin_files_max_scroll(&self) -> Option<f32> {
        let w = self.skin_files.as_ref()?;
        Some(skin_tab::visible_rows(&w.entries, &w.expanded).len().saturating_sub(skin_files_rows_visible()) as f32)
    }

    // Kaestchen einer Zeile; links davon steht bei Ordnern der Pfeil zum Aufklappen.
    pub(crate) fn skin_files_check_rect(row: Rect, depth: usize) -> Rect {
        Rect { x: row.x + 8.0 + depth as f32 * SKIN_FILES_INDENT + 22.0, y: row.y + ((row.h - 18.0) / 2.0).round(), w: 18.0, h: 18.0 }
    }

    // Linke Taste, waehrend das Dateifenster offen ist. Immer verbraucht.
    pub(crate) fn skin_files_press(&mut self) -> bool {
        if self.skin_files.is_none() {
            return false;
        }
        let (x, y) = self.cursor_pos;
        let (p, _, alles, fertig) = skin_files_layout();
        if fertig.contains(x, y) || !p.contains(x, y) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.skin_files = None;
            return true;
        }
        let Some(w) = self.skin_files.as_ref() else { return true };
        let skin = w.skin.clone();
        if alles.contains(x, y) {
            self.play_ui_sound(ui_sound::UiSound::Click);
            self.settings.skin_exclusions.remove(&skin);
            self.sync_skin_files_mirror();
            self.persist_settings();
            return true;
        }
        let Some((i, zeile)) = self.skin_files_rows().into_iter().find(|(_, r)| r.contains(x, y)) else { return true };
        let Some(w) = self.skin_files.as_mut() else { return true };
        let e = w.entries[i].clone();
        let kasten = Self::skin_files_check_rect(zeile, e.depth);
        // Ordner: das Kaestchen schaltet, der Rest der Zeile klappt auf und zu.
        if e.is_dir && !(kasten.x - 4.0..=kasten.x + kasten.w + 4.0).contains(&x) {
            if !w.expanded.remove(&e.path) {
                w.expanded.insert(e.path);
            }
            self.play_ui_sound(ui_sound::UiSound::Step);
            return true;
        }
        let mut aus = self.settings.skin_exclusions.remove(&skin).unwrap_or_default();
        skin_tab::toggle(&mut aus, &w.entries, &e.path);
        // Kaestchen klingen wie Schalter: an oder aus, je nachdem, wie es jetzt steht.
        let an = !skin_tab::is_off(&aus, &e.path);
        if !aus.is_empty() {
            self.settings.skin_exclusions.insert(skin, aus);
        }
        self.sync_skin_files_mirror();
        self.play_ui_sound(if an { ui_sound::UiSound::ToggleOn } else { ui_sound::UiSound::ToggleOff });
        self.persist_settings();
        true
    }

    // `weg` Zeilen Richtung Ende (s. App::wheel_units).
    pub(crate) fn scroll_skin_files(&mut self, weg: f32) {
        let Some(max) = self.skin_files_max_scroll() else { return };
        if let Some(w) = self.skin_files.as_mut() {
            w.scroll.push(weg, SKIN_FILES_ROWS_PER_NOTCH, max);
        }
    }

    // Mausrad im Skin-Tab: die Spalte unter dem Zeiger, `weg` Kacheln Richtung Ende.
    pub(crate) fn scroll_skin_column(&mut self, weg: f32) {
        use skin_tab::Column;
        let (x, y) = self.pointer();
        let Some(c) = [Column::Available, Column::Selected].into_iter().find(|&c| skin_column_rect(c).contains(x, y)) else { return };
        let max = self.skin_max_scroll(c);
        self.skin_scroll[usize::from(c == Column::Selected)].push(weg, 1.0, max);
    }

    // Rand rechts im Skin-Tab: "Ordner öffnen" und "Neu laden", darunter der Schalter Zips automatisch entpacken.
    // Bildschirmkoordinaten. Liefert die Knoepfe, wo die Beschriftung der Auswahl steht und
    // wo darunter die Erklaerung beginnen kann.
    pub(crate) fn skin_strip(&self, gpu: &GpuState) -> (Vec<Button>, (f32, f32), f32) {
        let (sw, _) = gpu.size();
        let rand = self.side_strip_w(sw);
        let x = sw - rand + 16.0;
        let w = rand - 32.0;
        let y0 = skin_column_rect(skin_tab::Column::Available).y + SETTINGS_CONTENT_DY;
        let neu_laden = Rect { x, y: y0 + SETTINGS_CTRL_H + 8.0, w, h: SETTINGS_CTRL_H };
        let mut out = vec![
            Self::nav_button(ButtonAction::OpenSkinFolder, Rect { x, y: y0, w, h: SETTINGS_CTRL_H }, "Ordner öffnen", UI_SCALE_SMALL),
            Self::nav_button(ButtonAction::ReloadSkins, neu_laden, "Neu laden", UI_SCALE_SMALL),
        ];
        let beschriftung = (x, neu_laden.y + SETTINGS_CTRL_H + 20.0);
        let wahl = Rect { x, y: beschriftung.1 + gpu.text_height(UI_SCALE_SMALL) + 8.0, w, h: SETTINGS_CTRL_H };
        // An/Aus wie ueberall ein einzelner Schalter.
        let mut schalter = Self::toggle(ButtonAction::ToggleSkinAutoUnzip, 0, self.settings.skin_auto_unzip);
        schalter.rect = Rect { w: TOGGLE_W, ..wahl };
        out.push(schalter);
        (out, beschriftung, wahl.y + wahl.h + 28.0)
    }
}
