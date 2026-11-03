// Account-Menue, Acc-Manager, Kontextmenue und Token-Export.

use super::*;

impl App {
    // Reihenfolge der gespeicherten Konten in Liste und Acc-Manager (s. accounts::display_order).
    pub(crate) fn account_order(&self) -> Vec<usize> {
        accounts::display_order(&self.accounts.list, |a| self.account_check(a), self.credentials.as_ref().map(|c| c.profile_id.as_str()))
    }

    // Kontenliste unter dem Konto-Knopf: zuerst das angemeldete Konto, dann die uebrigen,
    // ungueltige zuletzt. Some = Index in accounts.list; None = ein angemeldetes Konto, das nicht
    // in der Liste steht. Ohne Anmeldung gibt es keine eigene Zeile, es ist nur keins markiert.
    pub(crate) fn account_menu_rows(&self, gpu: &GpuState) -> Vec<(Rect, Option<usize>)> {
        let l = self.topbar_layout(gpu);
        let w = l.konto.w.max(340.0);
        let x = l.konto.x + l.konto.w - w;
        let mut zeilen = Vec::new();
        if self.credentials.as_ref().is_some_and(|c| self.accounts.find(&c.profile_id).is_none()) {
            zeilen.push(None);
        }
        zeilen.extend(self.account_order().into_iter().map(Some));
        zeilen.into_iter().enumerate().map(|(n, z)| (Rect { x, y: TOPBAR_H + 4.0 + n as f32 * 48.0, w, h: 44.0 }, z)).collect()
    }

    // Kopf eines Kontos in `bild`: Gesicht + Hutlage aus dem Skin; waehrend der Pruefung der
    // Ladekreis an seiner Stelle, ein ungueltiges Konto blass.
    pub(crate) fn push_account_head(&self, a: Option<&accounts::Account>, uuid: Option<[u8; 16]>, bild: Rect, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>) {
        use theme::Slot;
        let g = theme::get;
        rects.push(UiRect::rounded(Rect { x: bild.x - 2.0, y: bild.y - 2.0, w: bild.w + 4.0, h: bild.h + 4.0 }, g(Slot::Border2), 4.0));
        // Ladepunkte nur, solange es noch keinen Kopf gibt -- eine erneute Pruefung laesst ihn stehen.
        let kopf_da = uuid.is_some_and(|k| self.skins.get(&k).is_some_and(|s| s.slot.is_some()));
        let pruefung = a.and_then(|a| self.account_checks.get(&accounts::key(&a.uuid))).filter(|(c, _)| *c == accounts::Check::Checking && !kopf_da).map(|(_, seit)| seit);
        // Auf dieses Konto wird gerade gewechselt: Ladepunkte, bis die Anmeldung durch ist.
        let wechsel = self.account_switching.as_ref().filter(|(k, _)| a.is_some_and(|a| accounts::key(&a.uuid) == *k)).map(|(_, seit)| seit);
        if let Some(seit) = wechsel.or(pruefung) {
            rects.push(UiRect::rounded(bild, g(Slot::SurfaceSunken), 3.0));
            rects.extend(loading_dots(bild, seit.elapsed().as_secs_f32(), g(Slot::Accent)));
            return;
        }
        let blass = a.is_some_and(|a| matches!(self.account_check(a), accounts::Check::Invalid(_)));
        let tint = if blass { [1.0, 1.0, 1.0, 0.35] } else { [1.0; 4] };
        let uuid = uuid.unwrap_or([0; 16]);
        for (px, py) in [(8.0, 8.0), (40.0, 8.0)] {
            sprites.push(UiSprite::quad(bild.x, bild.y, bild.w, bild.h, self.skin_face_uv(&uuid, px, py), tint, SpriteAtlas::Entity));
        }
    }

    pub(crate) fn build_account_menu(&self, overlay: &mut UiOverlay) {
        let Some(gpu) = self.gpu.as_ref().filter(|_| self.account_menu_open && self.chrome_visible()) else { return };
        use theme::Slot;
        let g = theme::get;
        let zeilen = self.account_menu_rows(gpu);
        let (Some(erste), Some(letzte)) = (zeilen.first(), zeilen.last()) else { return };
        let panel = Rect { x: erste.0.x - 6.0, y: erste.0.y - 6.0, w: erste.0.w + 12.0, h: letzte.0.y + letzte.0.h - erste.0.y + 12.0 };
        let mut rects = vec![UiRect::shadow(panel, 8.0, 8.0, 24.0, 0.5), UiRect::framed(panel, g(Slot::Surface1), 8.0, 1.0, g(Slot::Border1))];
        let mut texts = Vec::new();
        let mut sprites = Vec::new();
        let (cx, cy) = self.cursor_pos;
        for (r, z) in zeilen {
            let konto = z.and_then(|i| self.accounts.list.get(i));
            let (name, aktiv, uuid) = match konto {
                None => (
                    self.credentials.as_ref().map_or_else(String::new, |c| c.username.clone()),
                    true,
                    self.credentials.as_ref().and_then(|c| uuid_bytes_from_hex(&c.profile_id)),
                ),
                Some(a) => (a.name.clone(), self.shown_as_active(a), uuid_bytes_from_hex(&a.uuid)),
            };
            let ungueltig = konto.is_some_and(|a| matches!(self.account_check(a), accounts::Check::Invalid(_)));
            // Weich ein- und ausgeblendet wie in den Aufklapplisten; hover_level meldet den
            // Hover auch fuer den Klang an.
            let k = self.hover_level(r, r.contains(cx, cy)).min(1.0);
            if aktiv {
                rects.push(UiRect::rounded(r, mix_color(g(Slot::AccentSurface), g(Slot::AccentSurfaceHover), k), 6.0));
            } else if k > 0.0 {
                rects.push(UiRect::rounded(r, theme::with_alpha(g(Slot::Surface2), k), 6.0));
            }
            // Kopf aus dem Skin wie oben in der Leiste; solange der Skin eines Kontos noch
            // laedt, schwarz-lila (s. request_account_heads).
            let bild = Rect { x: r.x + 8.0, y: r.y + 6.0, w: 32.0, h: 32.0 };
            self.push_account_head(konto, uuid, bild, &mut rects, &mut sprites);
            let ty = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            let farbe = if aktiv && !ungueltig {
                g(Slot::AccentText)
            } else if ungueltig {
                theme::text_3()
            } else {
                theme::text()
            };
            texts.push(UiText { text: name, x: bild.x + bild.w + 12.0, y: ty, color: farbe, scale: UI_SCALE_SMALL });
            if let Some(a) = konto {
                self.push_account_info(gpu, a, r, &mut texts);
            }
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites, pivot: (0.0, 0.0), angle: 0.0, clip: None });
    }

    // Linke Taste bei offener Kontenliste. true = verbraucht (Klick in der Liste oder daneben).
    pub(crate) fn account_menu_press(&mut self) -> bool {
        let Some(gpu) = self.gpu.as_ref() else { return false };
        let (cx, cy) = self.cursor_pos;
        // Der Knopf selbst (jetzt Acc-Manager) laeuft normal ueber buttons().
        if self.topbar_layout(gpu).konto.contains(cx, cy) {
            return false;
        }
        let treffer = self.account_menu_rows(gpu).into_iter().find(|(r, _)| r.contains(cx, cy));
        self.account_menu_open = false;
        if let Some((_, Some(i))) = treffer {
            self.play_ui_sound(ui_sound::UiSound::Click);
            self.use_account(i);
        }
        true
    }

    // Acc-Manager: alle Teile des Fensters, fuer Zeichnen und Klicks aus EINER Quelle.
    pub(crate) fn account_manager_layout(&self, gpu: &GpuState) -> (Rect, Vec<AccElem>) {
        let (sw, sh) = gpu.size();
        let w = 600.0;
        let pad = 24.0;
        let innen = w - 2.0 * pad;
        let mut el = Vec::new();
        let mut y = 20.0 + gpu.text_height(UI_SCALE_NORMAL) + 12.0 + gpu.text_height(UI_SCALE_SMALL) + 24.0;
        // Zwei Gruppen mit eigener Ueberschrift, leere fallen weg: echte Konten (erneuern sich
        // selbst) und eingefuegte Tokens (laufen ab). Ein Klick meldet an, ein Rechtsklick
        // oeffnet "Ausloggen"/"Account vergessen".
        let ordnung = self.account_order();
        let fremd = self.credentials.as_ref().filter(|c| self.accounts.find(&c.profile_id).is_none());
        let mut erste = true;
        for (quelle, titel) in [(accounts::Source::Microsoft, "KONTEN"), (accounts::Source::Token, "TOKENS")] {
            let zeilen: Vec<usize> = ordnung.iter().copied().filter(|&i| self.accounts.list[i].source == quelle).collect();
            let konten = quelle == accounts::Source::Microsoft;
            // Ohne jedes Konto steht "Konten" mit dem Hinweis da.
            let leer = ordnung.is_empty() && fremd.is_none();
            if zeilen.is_empty() && !(konten && (fremd.is_some() || leer)) {
                continue;
            }
            if !erste {
                y += 12.0;
            }
            erste = false;
            el.push(AccElem { rect: Rect { x: pad, y, w: innen, h: gpu.text_height(TOPBAR_SUB_SCALE) }, action: None, label: titel.into(), kind: AccKind::Header });
            y += gpu.text_height(TOPBAR_SUB_SCALE) + 8.0;
            // Angemeldet, aber (noch) nicht in der Liste: als eigene Zeile ohne Aktion.
            if let Some(c) = fremd.filter(|_| konten) {
                el.push(AccElem { rect: Rect { x: pad, y, w: innen, h: 48.0 }, action: None, label: c.username.clone(), kind: AccKind::Current });
                y += 52.0;
            }
            for i in zeilen {
                let a = &self.accounts.list[i];
                let kind = if self.shown_as_active(a) { AccKind::Current } else { AccKind::Other };
                el.push(AccElem { rect: Rect { x: pad, y, w: innen, h: 48.0 }, action: Some(ButtonAction::UseAccount(i)), label: a.name.clone(), kind });
                y += 52.0;
            }
            if konten && leer {
                el.push(AccElem { rect: Rect { x: pad, y, w: innen, h: 40.0 }, action: None, label: "Noch kein Konto. Mit Microsoft anmelden.".into(), kind: AccKind::Hint });
                y += 44.0;
            }
        }
        y += 16.0;
        // Anmelden: Microsoft ueber die ganze Breite, darunter die beiden Token-Wege nebeneinander.
        let halb = (innen - 12.0) / 2.0;
        let knopf = |x: f32, y: f32, w: f32, action, label: &str, kind| AccElem { rect: Rect { x, y, w, h: 40.0 }, action: Some(action), label: label.into(), kind };
        el.push(knopf(pad, y, innen, ButtonAction::MicrosoftLogin, "Mit Microsoft anmelden", AccKind::Primary));
        y += 48.0;
        el.push(knopf(pad, y, halb, ButtonAction::OpenInputField(InputKind::RefreshToken), "Refresh-Token", AccKind::Button));
        el.push(knopf(pad + halb + 12.0, y, halb, ButtonAction::OpenInputField(InputKind::Token), "Token eingeben", AccKind::Button));
        y += 48.0;
        // Offenes Eingabefeld (Token oder Refresh-Token) mit OK.
        if matches!(self.input_field, Some((InputKind::Token | InputKind::RefreshToken, _))) {
            y += gpu.text_height(TOPBAR_SUB_SCALE) + 8.0;
            el.push(AccElem { rect: Rect { x: pad, y, w: innen - 100.0, h: 40.0 }, action: None, label: String::new(), kind: AccKind::Field });
            el.push(AccElem { rect: Rect { x: pad + innen - 88.0, y, w: 88.0, h: 40.0 }, action: Some(ButtonAction::SubmitInputField), label: "OK".into(), kind: AccKind::Primary });
            y += 48.0;
        }
        y += 8.0;
        el.push(AccElem { rect: Rect { x: pad + innen - 140.0, y, w: 140.0, h: 40.0 }, action: None, label: "Schließen".into(), kind: AccKind::Close });
        let h = y + 40.0 + pad;
        let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round().max(TOPBAR_H + 8.0), w, h };
        for e in &mut el {
            e.rect.x += p.x;
            e.rect.y += p.y;
        }
        (p, el)
    }

    pub(crate) fn build_account_manager(&self, overlay: &mut UiOverlay) {
        let Some(gpu) = self.gpu.as_ref().filter(|_| self.account_manager_open && self.mode == AppMode::Menu) else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (p, el) = self.account_manager_layout(gpu);
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
        ];
        let mut sprites = Vec::new();
        let mut texts = vec![UiText { text: "Acc-Manager".into(), x: p.x + 24.0, y: p.y + 20.0, color: theme::text(), scale: UI_SCALE_NORMAL }];
        let (status, farbe) = self.account_status_line();
        let status_y = p.y + 20.0 + gpu.text_height(UI_SCALE_NORMAL) + 12.0;
        texts.push(UiText { text: status, x: p.x + 24.0, y: status_y, color: farbe, scale: UI_SCALE_SMALL });
        let mitte_y = |r: Rect, scale: f32| (r.y + (r.h - gpu.text_height(scale)) / 2.0).round();
        for e in &el {
            let hover = e.rect.contains(cx, cy);
            match e.kind {
                AccKind::Current | AccKind::Other => {
                    let an = e.kind == AccKind::Current;
                    let konto = match e.action {
                        Some(ButtonAction::UseAccount(i)) => self.accounts.list.get(i),
                        _ => None,
                    };
                    let ungueltig = konto.is_some_and(|a| matches!(self.account_check(a), accounts::Check::Invalid(_)));
                    let fill = if an { g(Slot::AccentSurface) } else if hover || self.account_context.is_some_and(|(i, _)| konto.is_some_and(|a| self.accounts.list.get(i) == Some(a))) { g(Slot::Surface2) } else { g(Slot::Surface1) };
                    rects.push(UiRect::framed(e.rect, fill, 6.0, 1.0, if an { g(Slot::Accent) } else { g(Slot::Border1) }));
                    let bild = Rect { x: e.rect.x + 8.0, y: e.rect.y + 8.0, w: 32.0, h: 32.0 };
                    let uuid = konto.map(|a| a.uuid.as_str()).or(self.credentials.as_ref().map(|c| c.profile_id.as_str())).and_then(uuid_bytes_from_hex);
                    self.push_account_head(konto, uuid, bild, &mut rects, &mut sprites);
                    let farbe = if ungueltig {
                        theme::text_3()
                    } else if an {
                        g(Slot::AccentText)
                    } else {
                        theme::text()
                    };
                    texts.push(UiText { text: e.label.clone(), x: bild.x + bild.w + 12.0, y: mitte_y(e.rect, UI_SCALE_SMALL), color: farbe, scale: UI_SCALE_SMALL });
                    if let Some(a) = konto {
                        self.push_account_info(gpu, a, e.rect, &mut texts);
                    }
                }
                AccKind::Hint => texts.push(UiText { text: e.label.clone(), x: e.rect.x, y: mitte_y(e.rect, UI_SCALE_SMALL), color: theme::text_3(), scale: UI_SCALE_SMALL }),
                AccKind::Header => texts.push(UiText { text: e.label.clone(), x: e.rect.x, y: e.rect.y, color: theme::text_3(), scale: TOPBAR_SUB_SCALE }),
                AccKind::Field => {
                    let Some((kind, wert)) = &self.input_field else { continue };
                    let label = match kind {
                        InputKind::Token => "MINECRAFT-TOKEN (STRG+V)",
                        _ => "MICROSOFT-REFRESH-TOKEN (STRG+V)",
                    };
                    texts.push(UiText { text: label.into(), x: e.rect.x, y: e.rect.y - gpu.text_height(TOPBAR_SUB_SCALE) - 8.0, color: theme::text_3(), scale: TOPBAR_SUB_SCALE });
                    rects.push(UiRect::framed(e.rect, g(Slot::SurfaceSunken), 6.0, 1.0, g(Slot::Accent)));
                    let gezeigt = token_display(gpu, wert, e.rect.w - 24.0);
                    let ty = mitte_y(e.rect, UI_SCALE_SMALL);
                    let x = e.rect.x + 12.0 + gpu.measure_raw(&gezeigt, UI_SCALE_SMALL);
                    self.push_caret(&mut rects, gpu.caret_rect(x, ty, UI_SCALE_SMALL));
                    push_typed_marker(&mut texts);
                    texts.push(UiText { text: gezeigt, x: e.rect.x + 12.0, y: ty, color: theme::text(), scale: UI_SCALE_SMALL });
                }
                AccKind::Button | AccKind::Primary | AccKind::Close => {
                    let farbe = match e.kind {
                        AccKind::Primary => g(Slot::Accent),
                        _ => g(Slot::Surface2),
                    };
                    let b = Button { slider: None, action: ButtonAction::Back, rect: e.rect, label: String::new(), enabled: true, color: farbe, scale: UI_SCALE_SMALL };
                    let (flaeche, textfarbe) = styled_button(&b, self.hover_level(b.rect, hover));
                    rects.push(flaeche);
                    texts.push(gpu.centered_text(e.rect, e.label.clone(), textfarbe, UI_SCALE_SMALL));
                }
            }
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites, pivot: (0.0, 0.0), angle: 0.0, clip: None });
    }

    // Rechts in einer Kontozeile: bei Tokens, die sich nicht selbst erneuern, wie lange sie
    // noch gelten; ein ungueltiges Konto sagt das statt dessen. Woher ein Konto stammt, steht
    // im Acc-Manager schon in der Ueberschrift seiner Gruppe.
    pub(crate) fn push_account_info(&self, gpu: &GpuState, a: &accounts::Account, zeile: Rect, texts: &mut Vec<UiText>) {
        let (text, farbe) = match self.account_check(a) {
            accounts::Check::Invalid(_) => ("ungültig".to_string(), theme::get(theme::Slot::Danger)),
            _ if a.source != accounts::Source::Microsoft => match accounts::expiry(&a.access_token) {
                Some(exp) => (accounts::remaining_text(exp, accounts::now_unix()), theme::text_2()),
                None => return,
            },
            _ => return,
        };
        let ty = (zeile.y + (zeile.h - gpu.text_height(TOPBAR_SUB_SCALE)) / 2.0).round();
        let x = zeile.x + zeile.w - 12.0 - gpu.measure_text(&text, TOPBAR_SUB_SCALE);
        texts.push(UiText { text, x: x.round(), y: ty, color: farbe, scale: TOPBAR_SUB_SCALE });
    }

    // Rechtsklick-Menue eines Kontos im Acc-Manager: am Zeiger, in den Bildschirm geschoben.
    // "Ausloggen" nur beim angemeldeten Konto.
    pub(crate) fn account_context_layout(&self) -> Option<(Rect, Vec<(Rect, AccountContextItem)>)> {
        let (i, at) = self.account_context?;
        let a = self.accounts.list.get(i)?;
        let mut items = Vec::new();
        if self.is_active_account(a) {
            items.push(AccountContextItem::Logout);
        }
        // Token-Export auskommentiert, vorerst abgeschaltet.
        // // Mit Refresh-Token wird ein neues geholt, ohne zeigt es das gespeicherte.
        // items.push(AccountContextItem::ExportToken);
        // if !a.refresh_token.is_empty() {
        //     items.push(AccountContextItem::ExportRefreshToken);
        // }
        items.push(AccountContextItem::Forget);
        let (sw, sh) = ui_screen();
        let h = items.len() as f32 * SERVER_CONTEXT_ROW_H + 12.0;
        // Token-Export auskommentiert, vorerst abgeschaltet. Dann wieder ACCOUNT_CONTEXT_W statt SERVER_CONTEXT_W.
        let x = at.0.min(sw - SERVER_CONTEXT_W - 8.0);
        let y = at.1.min(sh - h - 8.0);
        let p = Rect { x, y, w: SERVER_CONTEXT_W, h };
        let zeilen = items.into_iter().enumerate().map(|(n, item)| (Rect { x: p.x + 6.0, y: p.y + 6.0 + n as f32 * SERVER_CONTEXT_ROW_H, w: p.w - 12.0, h: SERVER_CONTEXT_ROW_H }, item)).collect();
        Some((p, zeilen))
    }

    pub(crate) fn account_context_open(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (cx, cy) = self.cursor_pos;
        let (_, el) = self.account_manager_layout(gpu);
        self.account_context = el.iter().find_map(|e| match e.action {
            Some(ButtonAction::UseAccount(i)) if e.rect.contains(cx, cy) => Some((i, (cx, cy))),
            _ => None,
        });
        if self.account_context.is_some() {
            self.play_ui_sound(ui_sound::UiSound::Open);
        }
    }

    // Linke Taste bei offenem Konto-Menue: Zeile ausfuehren, daneben schliesst es nur.
    pub(crate) fn account_context_press(&mut self) {
        let Some((_, zeilen)) = self.account_context_layout() else { return };
        let Some((i, _)) = self.account_context.take() else { return };
        let (cx, cy) = self.cursor_pos;
        match zeilen.into_iter().find(|(r, _)| r.contains(cx, cy)).map(|(_, item)| item) {
            Some(AccountContextItem::Logout) => {
                self.play_ui_sound(ui_sound::UiSound::Click);
                self.logout();
            }
            // Token-Export auskommentiert, vorerst abgeschaltet.
            // Some(AccountContextItem::ExportToken) => {
            //     self.play_ui_sound(ui_sound::UiSound::Open);
            //     self.start_token_export(i, false);
            // }
            // Some(AccountContextItem::ExportRefreshToken) => {
            //     self.play_ui_sound(ui_sound::UiSound::Open);
            //     self.start_token_export(i, true);
            // }
            Some(AccountContextItem::Forget) => {
                self.play_ui_sound(ui_sound::UiSound::Close);
                self.forget_account(i);
            }
            None => {}
        }
    }

    // Token-Export auskommentiert, vorerst abgeschaltet.
    // // Token-Export: ueber das Refresh-Token ein neues Minecraft-Token holen. Das Fenster mit
    // // der Warnung steht sofort, das Token zeigt es erst auf "Anzeigen". Geht nur mit einer
    // // Client-ID, die Microsoft annimmt (die eigene erst nach Mojangs Freigabe).
    // pub(crate) fn start_token_export(&mut self, i: usize, refresh: bool) {
    //     let Some(a) = self.accounts.list.get(i).cloned() else { return };
    //     self.account_manager_open = false;
    //     // Reines Token-Konto: nichts zu erneuern, das gespeicherte Token ist alles, was es gibt.
    //     if a.refresh_token.is_empty() {
    //         self.token_export = Some(TokenExport::Ready { name: a.name, refresh: false, token: a.access_token, client_id: None });
    //         return;
    //     }
    //     self.token_export = Some(TokenExport::Busy { name: a.name.clone(), refresh });
    //     let tx = self.auth_tx.clone();
    //     let key = accounts::key(&a.uuid);
    //     self.tokio_rt.spawn(async move {
    //         let result = async {
    //             let msa = auth::refresh_msa(&a.client_id, &a.refresh_token).await.map_err(|e| e.to_string())?;
    //             let c = auth::msa_to_minecraft(&msa.access_token).await.map_err(|e| e.to_string())?;
    //             Ok((c, (msa.refresh_token, a.client_id.clone())))
    //         }
    //         .await;
    //         let _ = tx.send(AuthEvent::TokenExported { key, result });
    //     });
    // }

    // // Fenster des Token-Exports: Rahmen, Zeilen (Text, Farbe, Faktor) und Knoepfe.
    // pub(crate) fn token_export_layout(&self, gpu: &GpuState) -> Option<(Rect, Vec<(String, [f32; 4], f32)>, Vec<(Rect, TokenExportButton, &'static str, bool)>)> {
    //     let export = self.token_export.as_ref()?;
    //     let (sw, sh) = ui_screen();
    //     let w = 640.0;
    //     let innen = w - 48.0;
    //     let mut zeilen: Vec<(String, [f32; 4], f32)> = Vec::new();
    //     let absatz = |text: &str, farbe: [f32; 4], zeilen: &mut Vec<(String, [f32; 4], f32)>| {
    //         for z in wrap_text(gpu, text, UI_SCALE_SMALL, innen) {
    //             zeilen.push((z, farbe, UI_SCALE_SMALL));
    //         }
    //     };
    //     let (name, art) = match export {
    //         TokenExport::Busy { name, refresh } | TokenExport::Ready { name, refresh, .. } | TokenExport::Shown { name, refresh, .. } => {
    //             (name, if *refresh { "Refresh-Token" } else { "Token" })
    //         }
    //         TokenExport::Failed { name, .. } => (name, "Token"),
    //     };
    //     zeilen.push((format!("{art} von {name}"), theme::text(), UI_SCALE_NORMAL));
    //     let knoepfe: Vec<(TokenExportButton, &'static str, bool)> = match export {
    //         TokenExport::Busy { .. } | TokenExport::Ready { .. } => {
    //             absatz(
    //                 if art == "Refresh-Token" {
    //                     "Achte darauf, dass niemand sonst auf deinen Bildschirm sieht -- auch kein Stream, keine Aufnahme und kein geteilter Bildschirm. Mit dem Refresh-Token kann sich jeder immer wieder als du anmelden, bis du es bei Microsoft zurückziehst."
    //                 } else {
    //                     "Achte darauf, dass niemand sonst auf deinen Bildschirm sieht -- auch kein Stream, keine Aufnahme und kein geteilter Bildschirm. Mit dem Token kann sich jeder als du anmelden, bis es abläuft."
    //                 },
    //                 theme::get(theme::Slot::Warning),
    //                 &mut zeilen,
    //             );
    //             let bereit = matches!(export, TokenExport::Ready { .. });
    //             let stand = match export {
    //                 TokenExport::Busy { .. } => format!("Neues {art} wird erzeugt ..."),
    //                 TokenExport::Ready { client_id: None, refresh: false, .. } => "Gespeichertes Token (das Konto hat kein Refresh-Token, ein neues gibt es nicht).".to_string(),
    //                 _ => format!("Neues {art} ist da."),
    //             };
    //             absatz(&stand, theme::text(), &mut zeilen);
    //             vec![(TokenExportButton::Cancel, "Abbrechen", true), (TokenExportButton::Show, "Anzeigen", bereit)]
    //         }
    //         TokenExport::Shown { token, client_id, .. } => {
    //             absatz(token, theme::text(), &mut zeilen);
    //             if let Some(exp) = accounts::expiry(token) {
    //                 absatz(&format!("Gültig: {}", accounts::remaining_text(exp, accounts::now_unix())), theme::text_2(), &mut zeilen);
    //             }
    //             // Ein Refresh-Token nimmt Microsoft nur mit der App an, die es ausgestellt hat.
    //             if let Some(id) = client_id {
    //                 absatz(&format!("Client-ID: {id}"), theme::text_2(), &mut zeilen);
    //             }
    //             vec![(TokenExportButton::Cancel, "Schließen", true), (TokenExportButton::Copy, "Kopieren", true)]
    //         }
    //         TokenExport::Failed { error, .. } => {
    //             absatz(&format!("Kein neues Token: {error}"), theme::get(theme::Slot::Danger), &mut zeilen);
    //             vec![(TokenExportButton::Cancel, "Schließen", true)]
    //         }
    //     };
    //     let text_h: f32 = zeilen.iter().map(|(_, _, s)| gpu.text_height(*s) + 6.0).sum();
    //     let h = (24.0 + text_h + 20.0 + 40.0 + 24.0).round();
    //     let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
    //     let mut x = p.x + p.w - 24.0;
    //     let mut rects = Vec::new();
    //     for (knopf, label, an) in knoepfe.into_iter().rev() {
    //         x -= 140.0;
    //         rects.push((Rect { x, y: p.y + p.h - 24.0 - 40.0, w: 140.0, h: 40.0 }, knopf, label, an));
    //         x -= 12.0;
    //     }
    //     rects.reverse();
    //     Some((p, zeilen, rects))
    // }

    // pub(crate) fn build_token_export(&self, overlay: &mut UiOverlay) {
    //     let Some(gpu) = self.gpu.as_ref().filter(|_| self.mode == AppMode::Menu) else { return };
    //     let Some((p, zeilen, knoepfe)) = self.token_export_layout(gpu) else { return };
    //     use theme::Slot;
    //     let g = theme::get;
    //     let (sw, sh) = gpu.size();
    //     let (cx, cy) = self.cursor_pos;
    //     let mut rects = vec![
    //         UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
    //         UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
    //         UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
    //     ];
    //     let mut texts = Vec::new();
    //     let mut y = p.y + 24.0;
    //     for (text, farbe, scale) in zeilen {
    //         let h = gpu.text_height(scale);
    //         texts.push(UiText { text, x: p.x + 24.0, y: y.round(), color: farbe, scale });
    //         y += h + 6.0;
    //     }
    //     for (r, _, label, an) in knoepfe {
    //         let b = Button {
    //             slider: None,
    //             action: ButtonAction::Nothing,
    //             rect: r,
    //             label: String::new(),
    //             enabled: an,
    //             color: g(Slot::Surface2),
    //             scale: UI_SCALE_SMALL,
    //         };
    //         let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, an && r.contains(cx, cy)));
    //         rects.push(flaeche);
    //         texts.push(gpu.centered_text(r, label.to_string(), farbe, UI_SCALE_SMALL));
    //     }
    //     overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
    // }

    // // Linke Taste bei offenem Token-Fenster. Immer verbraucht; daneben schliesst es nicht --
    // // das Token soll nicht durch einen Fehlklick verschwinden, bevor es kopiert ist.
    // pub(crate) fn token_export_press(&mut self) {
    //     let Some(gpu) = self.gpu.as_ref() else { return };
    //     let Some((_, _, knoepfe)) = self.token_export_layout(gpu) else { return };
    //     let (cx, cy) = self.cursor_pos;
    //     let Some((_, knopf, _, _)) = knoepfe.into_iter().find(|(r, _, _, an)| *an && r.contains(cx, cy)) else { return };
    //     match knopf {
    //         TokenExportButton::Cancel => {
    //             self.play_ui_sound(ui_sound::UiSound::Close);
    //             self.token_export = None;
    //         }
    //         TokenExportButton::Show => {
    //             self.play_ui_sound(ui_sound::UiSound::Open);
    //             if let Some(TokenExport::Ready { name, refresh, token, client_id }) = self.token_export.take() {
    //                 self.token_export = Some(TokenExport::Shown { name, refresh, token, client_id });
    //             }
    //         }
    //         TokenExportButton::Copy => {
    //             self.play_ui_sound(ui_sound::UiSound::Click);
    //             let token = match &self.token_export {
    //                 Some(TokenExport::Shown { token, .. }) => token.clone(),
    //                 _ => return,
    //             };
    //             if let Some(cb) = self.clipboard() {
    //                 let _ = cb.set_text(token);
    //             }
    //             self.notify(notifications::Notification::info("Token kopiert", "Liegt jetzt in der Zwischenablage."));
    //         }
    //     }
    // }

    pub(crate) fn build_account_context(&self, overlay: &mut UiOverlay) {
        let Some((gpu, (p, zeilen))) = self.gpu.as_ref().zip(self.account_context_layout()).filter(|_| self.account_manager_open && self.mode == AppMode::Menu) else { return };
        use theme::Slot;
        let g = theme::get;
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![UiRect::shadow(p, 8.0, 8.0, 24.0, 0.5), UiRect::framed(p, g(Slot::Surface1), 8.0, 1.0, g(Slot::Border1))];
        let mut texts = Vec::new();
        for (r, item) in zeilen {
            let (label, farbe) = match item {
                AccountContextItem::Logout => ("Ausloggen", theme::text()),
                // Token-Export auskommentiert, vorerst abgeschaltet.
                // AccountContextItem::ExportToken => ("Token exportieren", theme::text()),
                // AccountContextItem::ExportRefreshToken => ("Refresh-Token exportieren", theme::text()),
                AccountContextItem::Forget => ("Account vergessen", g(Slot::Danger)),
            };
            if r.contains(cx, cy) {
                rects.push(UiRect::rounded(r, g(Slot::Surface2), 6.0));
            }
            let ty = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: r.x + 12.0, y: ty, color: farbe, scale: UI_SCALE_SMALL });
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
    }

    // Linke Taste bei offenem Acc-Manager. Immer verbraucht; daneben schliesst er.
    pub(crate) fn account_manager_press(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (p, el) = self.account_manager_layout(gpu);
        let (cx, cy) = self.cursor_pos;
        if !p.contains(cx, cy) {
            self.account_manager_open = false;
            return;
        }
        // Spaetere Elemente liegen oben.
        let Some(e) = el.into_iter().rev().find(|e| e.rect.contains(cx, cy)) else { return };
        self.play_ui_sound(ui_sound::UiSound::Click);
        match (e.kind, e.action) {
            (AccKind::Close, _) => self.account_manager_open = false,
            (_, Some(action)) => self.run_menu_action(action),
            _ => {}
        }
    }
}
