// Die Einstellungsseiten: Allgemein, Video, Steuerung, Design, Schrift, Audio, Skins.

use crate::*;

impl App {
    // Einstellungen-Tab: Beschriftung links, Bedienelement rechts. Die Bedienelemente
    // selbst kommen aus buttons(); hier nur Text und Wertfelder.
    // Kategorie-Auswahl: Titel + eine Kachel je SETTINGS_CATEGORIES-Eintrag (Buttons selbst
    // kommen aus buttons(), hier nur die Ueberschrift).
    pub(crate) fn build_settings_hub(&self, gpu: &GpuState, texts: &mut Vec<UiText>) {
        let title = "Einstellungen";
        texts.push(UiText { text: title.to_string(), x: UI_MARGIN, y: 90.0, color: theme::text(), scale: UI_SCALE_TITLE });
        // Welche Config gerade bearbeitet wird (gewaehlt im Launcher) -- rechts neben dem Titel
        // auf dessen Grundlinie, darunter beginnen schon die Kacheln.
        let x = UI_MARGIN + gpu.measure_text(title, UI_SCALE_TITLE) + 24.0;
        let y = 90.0 + gpu.text_height(UI_SCALE_TITLE) - gpu.text_height(UI_SCALE_SMALL);
        texts.push(UiText { text: format!("Config: {}", active_config()), x, y: y.round(), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
    }

    // Werte des Abschnitts System (Zeile, Text). Speicher wird hoechstens einmal je Sekunde
    // gelesen; der Compositor alle 3 s in einem eigenen Thread (X-Verbindung, gsettings), das
    // Bild zeigt solange den letzten Stand.
    pub(crate) fn system_rows(&self, gpu: &GpuState) -> [(usize, String); 5] {
        static CPU: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        let jetzt = Instant::now();
        let unbekannt = || i18n::tr("unbekannt").into_owned();
        let speicher = match self.memory_info.get() {
            Some((t, m)) if jetzt.duration_since(t) < Duration::from_secs(1) => m,
            _ => {
                let m = system_info::memory();
                self.memory_info.set(Some((jetzt, m)));
                m
            }
        };
        if self.compositing_checked.get().is_none_or(|t| jetzt.duration_since(t) >= Duration::from_secs(3)) {
            self.compositing_checked.set(Some(jetzt));
            let ziel = std::sync::Arc::clone(&self.compositing_info);
            std::thread::spawn(move || {
                let c = system_info::compositing();
                if let Ok(mut g) = ziel.lock() {
                    *g = c;
                }
            });
        }
        // Was mit unserem Fenster passiert: setzt der Compositor es zusammen, oder kommt es
        // direkt auf den Schirm -- und dann per Hardware-Flip oder als Kopie des X-Servers.
        let compositor = self.compositing_info.lock().ok().and_then(|g| g.clone());
        let fenster = self.window_path.as_ref().and_then(|w| w.lock().ok().map(|g| *g));
        let compositing = match (compositor, fenster) {
            (Some(system_info::Compositing::Always), _) => {
                let dwm = i18n::tr("DWM läuft, Umgehung nicht geprüft").into_owned();
                // Windows: wo die Presents landen (s. vblank::note_present).
                #[cfg(windows)]
                // In ms ab Zeile 0 wie Tear timing, mit der Lage der Luecke dahinter.
                let dwm = match vblank::present_phase() {
                    Some(p) => format!(
                        "{dwm} · Present {:.2} ms ±{:.2} ({} {:.2}–{:.2})",
                        p.mitte_ms,
                        (p.oben_ms - p.unten_ms) / 2.0,
                        i18n::tr("Lücke"),
                        p.sichtbar_ms,
                        p.periode_ms
                    ),
                    None => dwm,
                };
                dwm
            }
            (c, Some(system_info::WindowPath { composited: Some(true), .. })) => {
                let name = match c {
                    Some(system_info::Compositing::On { name: Some(n) }) => n,
                    _ => "Compositor".to_string(),
                };
                format!("{} {name}", i18n::tr("zusammengesetzt von"))
            }
            (_, Some(f)) => {
                let weg = i18n::tr(match f.last_present {
                    Some(system_info::PresentPath::Flip) => "Hardware-Flip",
                    Some(system_info::PresentPath::Copy) => "Kopie (X-Server)",
                    Some(system_info::PresentPath::SuboptimalCopy) => "Kopie, Flip nicht möglich",
                    Some(system_info::PresentPath::Skip) => "übersprungen",
                    None => "direkt",
                });
                match self.tear_readout(&f) {
                    Some(riss) => format!("{weg} · {riss}"),
                    None => weg.into_owned(),
                }
            }
            (Some(system_info::Compositing::Off), None) => i18n::tr("aus").into_owned(),
            (Some(system_info::Compositing::On { name }), None) => name.unwrap_or_else(|| i18n::tr("an").into_owned()),
            (None, None) => unbekannt(),
        };
        let ram = speicher.map_or_else(unbekannt, |m| system_info::format_used(m.ram_used, m.ram_total));
        let swap = match speicher {
            Some(m) if m.swap_total == 0 => i18n::tr("keiner").into_owned(),
            Some(m) => system_info::format_used(m.swap_used, m.swap_total),
            None => unbekannt(),
        };
        let cpu = CPU.get_or_init(system_info::cpu_name).clone().unwrap_or_else(unbekannt);
        [(CPU_ROW, cpu), (GPU_ROW, gpu.gpu_label()), (RAM_ROW, ram), (SWAP_ROW, swap), (COMPOSITING_ROW, compositing)]
    }

    // Wo der Riss der letzten Bilder lag, aus den Meldungen des X-Servers und den VBlanks (s.
    // system_info::tear_position). Steht er bei jeder Einstellung an derselben Stelle, haelt der
    // X-Server die Bilder fest -- dann hilft nur ein Neustart von X.
    pub(crate) fn tear_readout(&self, f: &system_info::WindowPath) -> Option<String> {
        let (letzter, periode) = self.vblank.as_ref()?.latest()?;
        let sichtbar = match self.vblank.as_ref().and_then(|v| v.lines) {
            Some((sichtbar, gesamt)) => periode * sichtbar as f64 / gesamt as f64,
            None => {
                let h = self.gpu.as_ref()?.size().1 as f64;
                periode * h / (h + 45.0)
            }
        };
        let (mitte, streuung) = system_info::tear_position(f, letzter.ts_ns, periode, sichtbar)?;
        Some(if streuung > 0.15 {
            i18n::tr("Riss wandert").into_owned()
        } else if mitte >= 1.0 {
            i18n::tr("Riss in der Austastlücke").into_owned()
        } else {
            format!("{} {:.0} %", i18n::tr("Riss bei"), mitte * 100.0)
        })
    }

    pub(crate) fn build_settings_general_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let set = &self.settings;
        for (row, label) in SETTINGS_LABELS.iter().enumerate() {
            let row_rect = Rect { x: SETTINGS_LABEL_X, y: settings_row_y(row), w: SETTINGS_CTRL_X, h: SETTINGS_CTRL_H };
            let y = (row_rect.y + (row_rect.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }

        // Wertfelder der Stepper (Mesh-Budget/Chunk-Limiter/Threads folgen unten).
        // Render-Ahead ist keine frei tippbare Zahl: mit 0..4 ist der Bereich so klein, dass die
        // Pfeile schneller sind (s. EditableValue::ALL-Kommentar).
        let frame_latency = if set.frame_latency == 0 { "0 (Fence)".to_string() } else { format!("{}", set.frame_latency) };
        let rect = stepper_value_rect(FRAME_LATENCY_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, frame_latency, theme::text(), UI_SCALE_SMALL));

        // FPS-Limit: bei selbsttaktenden Present-Modi ehrlicher als eine Zahl, die
        // gerade nichts bewirkt -- dann auch nicht bearbeitbar (der Stepper blendet dort
        // ebenfalls ab, s. buttons()).
        if gpu.present_mode_self_paces() {
            let rect = stepper_value_rect(FPS_LIMIT_ROW);
            rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(rect, "durch VBlank".to_string(), theme::text_disabled(), UI_SCALE_SMALL));
        } else {
            let fps = fps_limit_label(set.fps_limit);
            self.draw_editable_value(gpu, stepper_value_rect(FPS_LIMIT_ROW), EditableValue::FpsLimit, fps, rects, texts);
        }

        // Mesh-Budget: anklickbar fuer einen eigenen Wert.
        let mesh = format!("{}", set.mesh_columns_per_frame);
        self.draw_editable_value(gpu, stepper_value_rect(MESH_BUDGET_ROW), EditableValue::MeshBudget, mesh, rects, texts);


        // Worker-Anzahl: bewusst NICHT frei tippbar -- der sinnvolle Bereich haengt an der
        // Kernzahl dieser Maschine, nicht am Geschmack (s. EditableValue::ALL).
        let threads = format!("{}", set.chunk_update_threads);
        let rect = stepper_value_rect(CHUNK_THREADS_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, threads, theme::text(), UI_SCALE_SMALL));

        // Abschnitt System: reine Information, kein Bedienelement -- breitere Felder als die
        // Stepper-Werte, GPU- und CPU-Namen sind lang ("AMD: AMD Radeon Graphics (...)" ~425px).
        for (row, wert) in self.system_rows(gpu) {
            // Mindestens 500, laengere Werte (Compositing samt Present-Lage) bekommen ihre Breite,
            // statt mit "..." abgeschnitten zu werden.
            let breite = (gpu.measure_text(&wert, UI_SCALE_SMALL) + 32.0).max(500.0);
            let feld = Rect { x: SETTINGS_CTRL_X, y: settings_row_y(row), w: breite, h: SETTINGS_CTRL_H };
            rects.push(UiRect { rect: feld, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(feld, wert, theme::text(), UI_SCALE_SMALL));
        }
        // Riss-Timing: Wert in Millisekunden, mit Vorzeichen.
        let rect = stepper_value_rect(TEAR_TIMING_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, format!("{:+.2} ms", set.tear_timing_us as f32 / 1000.0), theme::text(), UI_SCALE_SMALL));
        // Riss-Reserve: ebenfalls in Millisekunden.
        let rect = stepper_value_rect(TEAR_RESERVE_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, format!("{:.2} ms", set.tear_reserve_us as f32 / 1000.0), theme::text(), UI_SCALE_SMALL));

        // TearFree zeigt seinen Wert direkt im Schalter selbst (AN/AUS/n.v./?), s.
        // tearfree_button() -- kein zusaetzliches Wertfeld noetig. Block-Rotation ist ein
        // Toggle-Button, s. buttons()/ToggleBlockRngRotation.
        // Ausserhalb von Linux gibt es keinen Schalter, nur den Hinweis -- als Feld wie die
        // System-Zeilen, nicht grau: graue Texte filtert build_menu_ui auf Einstellungsseiten weg.
        #[cfg(not(target_os = "linux"))]
        {
            let feld = Rect { x: SETTINGS_CTRL_X, y: settings_row_y(TEARFREE_ROW), w: STEPPER_VALUE_W, h: SETTINGS_CTRL_H };
            rects.push(UiRect { rect: feld, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(feld, i18n::tr("nur Linux und AMD").into_owned(), theme::text(), UI_SCALE_SMALL));
        }

    }

    // Generischer Renderer fuer eine Platzhalter-Kategorie (Video/Audio/Chat/
    // Barrierefreiheit): nur Beschriftung + abgeblendeter, nicht klickbarer Wert je Zeile.
    // Die eigentliche Erklaerung kommt als Hover-Tooltip (s. hovered_tooltip), nicht als
    // Dauertext -- deshalb hier keine weitere Beschriftung noetig.
    // `row_offset` laesst Platz fuer echte (nicht-Platzhalter-)Zeilen davor -- s.
    // build_settings_controls_tab, wo Maus-Empfindlichkeit Zeile 0 belegt und die
    // Platzhalter erst ab Zeile 1 folgen.
    // Video: Zeile 0 = FOV (echter Stepper, Wertfeld hier + Pfeil-Buttons aus buttons()),
    // Zeile 1 = Projektion (Umschalt-Button aus buttons(), hier nur das Label davor).
    pub(crate) fn build_settings_video_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        // Linke Spalte: Sichtfeld je Perspektive + Projektion. CMAA2 war hier zwischenzeitlich
        // geplant, ist aber deaktiviert -- s. CMAA2_ROW-Kommentar bei PROJECTION_ROW.
        let mut left: Vec<(usize, &str)> = Perspective::ALL.iter().map(|p| (p.fov_row(), p.fov_label())).collect();
        left.push((PROJECTION_ROW, "Projektion"));
        left.push((SMOOTH_LIGHTING_ROW, "Weiche Beleuchtung"));
        left.push((BLOCK_ROTATION_ROW, "Block-Rotation"));
        left.push((PARTICLES_ROW, "Partikel"));
        left.push((ARMOR_TRIMS_ROW, "Rüstungsbesatz"));
        for (row, label) in left {
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        // Rechte Spalte: Kameraabstand + -hoehe der beiden 3.-Person-Ansichten.
        for p in Perspective::THIRD_PERSON {
            for (row, label) in [(p.distance_row(), p.distance_label()), (p.y_offset_row(), p.y_offset_label())] {
                let Some(row) = row else { continue };
                let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
            }
        }
        // Staerke der weichen Beleuchtung (Pfeile aus buttons()); 0 % heisst aus.
        let rect = stepper_value_rect(SMOOTH_LIGHTING_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        let staerke = match self.settings.smooth_lighting_percent {
            0 => "Aus".to_string(),
            p => format!("{p} %"),
        };
        texts.push(gpu.centered_text(rect, staerke, theme::text(), UI_SCALE_SMALL));
        // FOV-Wertfelder (Pfeile kommen aus buttons()), Zahl ohne Nachkommastelle -- jedes
        // anklickbar fuer einen eigenen Wert (s. EditableValue::Fov), weil die 5er-Schritte
        // des Steppers z.B. eine 103 nie treffen. Die aktive Perspektive wird hervorgehoben,
        // sonst raet man, welche der drei Zeilen gerade wirkt.
        for p in Perspective::ALL {
            let fov = format!("{} °", self.settings.fov_of(p) as i32);
            self.draw_editable_value(gpu, stepper_value_rect(p.fov_row()), EditableValue::Fov(p), fov, rects, texts);
            // Kameraabstand und -hoehe in der rechten Spalte, in Bloecken mit Nachkomma-
            // stellen (die 0.5er- bzw. 0.25er-Schritte waeren als ganze Zahl nicht ablesbar).
            if let Some(row) = p.distance_row() {
                let d = format!("{:.1} Blöcke", self.settings.camera_distance_of(p));
                self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::CameraDistance(p), d, rects, texts);
            }
            if let Some(row) = p.y_offset_row() {
                let y = format!("{:+.2} Blöcke", self.settings.camera_y_offset_of(p));
                self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::CameraYOffset(p), y, rects, texts);
            }
        }
        // Schadens-Faerbung, rechte Spalte unter der Kamera-Gruppe: Farbfeld, Nur-Skin-Schalter
        // und Dauer.
        let y = (settings_row_y(DAMAGE_TINT_FIRST_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Schadensfarbe".to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        for (row, label) in [(DAMAGE_TINT_SKIN_ONLY_ROW, "Nur Skin färben"), (DAMAGE_TINT_TICKS_ROW, "Färbedauer")] {
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        let rect = stepper_value_rect(DAMAGE_TINT_TICKS_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, damage_tint_duration_text(self.settings.damage_tint_ticks), theme::text(), UI_SCALE_SMALL));

        self.build_video_glint_and_crit(gpu, rects, texts);

        // Sichtweiten: Render-Distance (anklickbar fuer einen eigenen Wert, das Maximum heisst
        // "unbegrenzt"), dann die Fernsicht.
        let set = &self.settings;
        for (row, label) in [
            (RENDER_DISTANCE_ROW, "Render-Distance"),
            (ENTITY_DISTANCE_ROW, "Entity-Renderdistanz"),
            (LOD_DETAIL_ROW, "LOD-Detail"),
        ] {
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        let weite = if set.max_render_distance >= MAX_RENDER_DISTANCE_MAX {
            "unbegrenzt".to_string()
        } else {
            format!("{} Chunks", set.max_render_distance)
        };
        self.draw_editable_value(gpu, stepper_value_rect(RENDER_DISTANCE_ROW), EditableValue::MaxRenderDistance, weite, rects, texts);
        let weite = set.entity_render_distance;
        let weite = if weite >= ENTITY_DISTANCE_MAX { "unbegrenzt".to_string() } else { format!("{weite} Blöcke") };
        self.draw_editable_value(gpu, stepper_value_rect(ENTITY_DISTANCE_ROW), EditableValue::EntityRenderDistance, weite, rects, texts);

        let lod_farbe = theme::text();
        for (row, text) in [
            (LOD_DETAIL_ROW, format!("{} px²", set.chunk_saver_threshold)),
        ] {
            let rect = stepper_value_rect(row);
            rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(rect, text, lod_farbe, UI_SCALE_SMALL));
        }
    }

    // Steuerung: zwei Spalten (s. Layout-Konstanten am Dateianfang). Links die
    // Tastenbelegungs-Tabelle (Kopfzeile + scrollbarer Zeilenbereich, Buttons selbst kommen
    // aus buttons() -- gemeinsamer Klick-Zustand, s. dortigen Kommentar). Rechts
    // Maus-Empfindlichkeit (Zeile 0) und das Verhalten darunter.
    pub(crate) fn build_settings_controls_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let set = &self.settings;
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        for (row, &zeile) in controls_rows().iter().enumerate() {
            let label = match zeile {
                ControlsRow::SensX => "Kamera-Empfindlichkeit X",
                ControlsRow::SensY => "Kamera-Empfindlichkeit Y",
                ControlsRow::SensSplit => "X/Y getrennt",
                ControlsRow::SensCalc => "Empfindlichkeits-Rechner",
                ControlsRow::CursorSens => "Zeiger-Empfindlichkeit",
                ControlsRow::CursorSize => "Zeigergröße",
                ControlsRow::CursorWidth => "Zeigerbreite",
                ControlsRow::CursorCircle => "Zeiger-Kreis",
                ControlsRow::SneakProtection => "Sneak-Kantenschutz",
                ControlsRow::SnapTap => "SnapTap (SOCD)",
                ControlsRow::JumpCooldown => "Sprung-Abklingzeit",
                ControlsRow::SprintOnStart => "Sprinten an bei Spielstart",
                ControlsRow::HoldUseRepeat => "Benutzen wiederholen",
                ControlsRow::RocketRepeat => "Raketen wiederholen",
                ControlsRow::ExtinguishDcGhost => "No Extinguish DC Ghost",
                ControlsRow::UseWhileMining => "Benutzen beim Abbauen",
                ControlsRow::CapsLock => "Feststelltaste sperren",
                ControlsRow::HoldConfirm => "Bestätigungsdauer (allgemein)",
                ControlsRow::ReconnectHold => "Reconnect-Verzögerung",
                ControlsRow::Bind(aktion) => aktion.short_label(),
            };
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
            // Wertfelder der Empfindlichkeiten (anklickbar fuer eine eigene Zahl, s.
            // EditableValue) und der beiden Zeiger-Regler.
            match zeile {
                ControlsRow::SensX => {
                    self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::MouseSensitivity, format_sensitivity(set.mouse_sensitivity), rects, texts)
                }
                ControlsRow::SensY => {
                    self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::MouseSensitivityY, format_sensitivity(set.mouse_sensitivity_y), rects, texts)
                }
                ControlsRow::CursorSens => self.draw_editable_value(
                    gpu,
                    stepper_value_rect(row),
                    EditableValue::CursorSensitivity,
                    format_cursor_sensitivity(set.cursor_sensitivity),
                    rects,
                    texts,
                ),
                ControlsRow::CursorSize | ControlsRow::CursorWidth => {
                    let wert = if zeile == ControlsRow::CursorSize { set.cursor_size } else { set.cursor_width };
                    let rect = stepper_value_rect(row);
                    rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(rect, format!("{wert} %"), theme::text(), UI_SCALE_SMALL));
                }
                ControlsRow::HoldConfirm | ControlsRow::ReconnectHold => {
                    let wert = if zeile == ControlsRow::HoldConfirm { set.hold_confirm_ms as i32 } else { set.reconnect_hold_ms };
                    let text = if wert < 0 { "Allgemein".to_string() } else { format!("{wert} ms") };
                    let rect = stepper_value_rect(row);
                    rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(rect, text, theme::text(), UI_SCALE_SMALL));
                }
                _ => {}
            }
        }
    }

    // Inventar-Tab: bisher genau eine echte Einstellung.
    // Chat-Tab: Beschriftungen und Wertfelder der sieben echten Zeilen (Pfeile aus buttons()),
    // darunter die Platzhalter.
    // Beschriftung und Wert der Chat-Einstellungen, Reihenfolge wie CHAT_TOOLTIPS (im
    // Einstellungsfenster des Chats im HUD-Editor, s. build_hud_config).
    pub(crate) fn chat_setting_rows(&self) -> [(&'static str, String); 7] {
        let set = &self.settings;
        [
            ("Textgröße", format!("{} %", set.chat_scale_percent)),
            ("Breite", format!("{}", set.chat_width)),
            ("Hintergrund", format!("{} %", set.chat_background)),
            ("Abdunkeln beim Tippen", format!("{} %", set.chat_open_dim)),
            ("Zeilen (im Spiel)", format!("{}", set.chat_lines)),
            ("Zeilen (beim Tippen)", format!("{}", set.chat_lines_open)),
            ("Ausblenden nach", if set.chat_fade_secs == 0 { "Nie".to_string() } else { format!("{} s", set.chat_fade_secs) }),
        ]
    }

    // Was alle Seiten mit settings_page gemeinsam haben: Linie unter den Reitern, Trennlinie
    // mit Namen vor jedem weiteren Abschnitt, Scrollleiste am rechten Rand, sobald nicht alle
    // Zeilen ins Fenster passen. Die Reiter selbst sind Knoepfe (s. section_tabs).
    pub(crate) fn build_settings_page_frame(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let Some(page) = self.screen.settings_page().filter(|_| self.mode == AppMode::Menu) else { return };
        let (screen_w, screen_h) = gpu.size();
        // Rechte Kante des Inhalts, wie fit_texts in build_menu_ui.
        let rechts = screen_w - self.side_strip_w(screen_w) - SETTINGS_CONTENT_DX - 16.0;
        let max_scroll = self.page_max_scroll();
        let linie_bis = if max_scroll > 0 { rechts - 18.0 } else { rechts };
        let linie = theme::get(theme::Slot::Border1);
        if page.sections.len() >= 2 {
            rects.push(UiRect { rect: Rect { x: SETTINGS_LABEL_X, y: SETTINGS_FIRST_Y - 5.0, w: linie_bis - SETTINGS_LABEL_X, h: 1.0 }, color: linie, ..UiRect::FLAT });
        }
        let erste = self.page_scroll().min(max_scroll);
        let name_ueber = |name: &str, linie_y: f32, texts: &mut Vec<UiText>| {
            let x = linie_bis - gpu.measure_text(name, UI_SCALE_SMALL);
            texts.push(UiText { text: name.to_string(), x: x.round(), y: (linie_y - 4.0 - gpu.text_height(UI_SCALE_SMALL)).round(), color: theme::text(), scale: UI_SCALE_SMALL });
        };
        // Der Abschnitt, in dem die oberste sichtbare Zeile liegt: sein Name ueber der Linie oben.
        if page.sections.len() >= 2 {
            if let Some(&(name, _)) = page.sections.iter().rev().find(|&&(_, row)| row <= erste) {
                name_ueber(name, SETTINGS_FIRST_Y - 5.0, texts);
            }
        }
        for &(name, row) in page.sections.iter().filter(|&&(_, row)| row > erste) {
            let y = settings_row_y(row);
            if y < 0.0 {
                continue;
            }
            // Mitten in der Luecke zwischen dem Feld darueber und dieser Zeile.
            let linie_y = (y - (SETTINGS_ROW_H - SETTINGS_CTRL_H + SETTINGS_SECTION_GAP) / 2.0).round();
            rects.push(UiRect { rect: Rect { x: SETTINGS_LABEL_X, y: linie_y, w: linie_bis - SETTINGS_LABEL_X, h: 1.0 }, color: linie, ..UiRect::FLAT });
            name_ueber(name, linie_y, texts);
        }
        if max_scroll > 0 {
            let hoehe = settings_rows_bottom(screen_h) - SETTINGS_FIRST_Y - (SETTINGS_ROW_H - SETTINGS_CTRL_H);
            let spur = Rect { x: rechts - 6.0, y: SETTINGS_FIRST_Y, w: 6.0, h: hoehe };
            rects.push(UiRect { rect: spur, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            let griff_h = (hoehe * (page.rows - max_scroll) as f32 / page.rows as f32).max(24.0);
            let anteil = self.page_scroll().min(max_scroll) as f32 / max_scroll as f32;
            let griff = Rect { x: spur.x, y: spur.y + (hoehe - griff_h) * anteil, w: spur.w, h: griff_h };
            rects.push(UiRect { rect: griff, color: theme::text_disabled(), ..UiRect::FLAT });
        }
    }

    // Aussehen: Hintergrundbild, darunter das Theme. Die Knoepfe kommen aus buttons().
    pub(crate) fn build_settings_design_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        self.build_theme_section(gpu, rects, texts);
        texts.push(UiText { text: "Spielbild abdunkeln".to_string(), x: SETTINGS_LABEL_X, y: zeile_y(MENU_BACKDROP_ROW), color: theme::text(), scale: UI_SCALE_SMALL });
        let abdunkeln = format!("{} %", self.settings.menu_backdrop_opacity);
        self.draw_editable_value(gpu, stepper_value_rect(MENU_BACKDROP_ROW), EditableValue::MenuBackdropOpacity, abdunkeln, rects, texts);
        texts.push(UiText { text: "Hintergrundbild".to_string(), x: SETTINGS_LABEL_X, y: zeile_y(WALLPAPER_ROW), color: theme::text(), scale: UI_SCALE_SMALL });
        // Rechts daneben, was gerade gilt. Beim eigenen Bild steht dort der Knopf (s.
        // buttons()), der Hinweis rechts davon.
        let hinweis_x = WALLPAPER_PICK_X + 240.0;
        match wallpaper::WallpaperMode::from_index(self.settings.wallpaper) {
            wallpaper::WallpaperMode::None => {}
            wallpaper::WallpaperMode::File => {
                let da = shared_config_dir().map(|d| wallpaper::stored_path(&d)).is_some_and(|p| p.is_file());
                let text = if da { "Bild gespeichert" } else { "Noch kein Bild gewählt" };
                texts.push(UiText { text: text.to_string(), x: hinweis_x, y: zeile_y(WALLPAPER_ROW), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
            }
            wallpaper::WallpaperMode::Desktop => {
                let text = match self.wallpaper_loader.current_path() {
                    Some(p) => p.display().to_string(),
                    None => "Kein Desktop-Hintergrund gefunden".to_string(),
                };
                texts.push(UiText { text, x: WALLPAPER_PICK_X, y: zeile_y(WALLPAPER_ROW), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
            }
            wallpaper::WallpaperMode::Coral => {
                if !hero_scene::path().is_some_and(|p| p.is_file()) {
                    let text = "Wird beim nächsten Start gerendert";
                    texts.push(UiText { text: text.to_string(), x: WALLPAPER_PICK_X, y: zeile_y(WALLPAPER_ROW), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
                }
            }
        }
    }

    // Theme-Abschnitt der Seite Aussehen: Kacheln links, Trennlinie, Farben rechts.
    pub(crate) fn build_theme_section(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        use theme::Slot;
        let g = theme::get;
        let set = &self.settings;
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let sichtbar = |y: f32| y > -1000.0;
        // Hinter einem offenen Fenster leuchtet nichts auf.
        let (qx, qy) = if self.theme_name_dialog.is_some() || self.theme_delete.is_some() { (-1.0, -1.0) } else { self.pointer() };
        let presets = theme::PRESETS.len();
        let anzahl = presets + set.custom_themes.len() + 1;
        for i in 0..anzahl {
            let r = theme_tile_rect(i);
            if !sichtbar(r.y) {
                continue;
            }
            let k = self.hover_level(r, r.contains(qx, qy)).min(1.0);
            if i == anzahl - 1 {
                // "+": ein neues eigenes Theme.
                rects.push(UiRect::framed(r, mix_color(g(Slot::Surface1), g(Slot::Surface2), k), 8.0, 1.0, mix_color(g(Slot::Border2), g(Slot::BorderStrong), k)));
                let farbe = mix_color(theme::text_2(), theme::text(), k);
                let (mx, my) = ((r.x + r.w / 2.0).round(), (r.y + r.h / 2.0).round());
                push_bar(rects, (mx - 10.0, my), (mx + 10.0, my), 3.0, farbe);
                push_bar(rects, (mx, my - 10.0), (mx, my + 10.0), 3.0, farbe);
                continue;
            }
            let (name, farben, wahl) = match i.checked_sub(presets) {
                None => (theme::PRESETS[i].name.to_string(), theme::PRESETS[i].colors, ThemeChoice::Preset(i)),
                Some(j) => (set.custom_themes[j].name.clone(), set.custom_themes[j].colors, ThemeChoice::Custom(j)),
            };
            // Die Kachel zeigt das Theme selbst: seine Flaeche, sein Akzent, seine Textfarbe.
            let c = |slot: Slot| theme::rgba(farben[slot.index()]);
            let gewaehlt = set.theme_choice == wahl;
            let rand = if gewaehlt { g(Slot::Accent) } else { mix_color(g(Slot::Border1), g(Slot::BorderStrong), k) };
            rects.push(UiRect::framed(r, c(Slot::BgPanel), 8.0, if gewaehlt { 2.0 } else { 1.0 }, rand));
            rects.push(UiRect::rounded(Rect { x: r.x + 10.0, y: r.y + 9.0, w: 6.0, h: r.h - 18.0 }, c(Slot::Accent), 3.0));
            for (n, slot) in [Slot::Surface2, Slot::AccentSurface, Slot::Text2].into_iter().enumerate() {
                let d = Rect { x: r.x + r.w - 14.0 - (3 - n) as f32 * 16.0, y: r.y + r.h / 2.0 - 6.0, w: 12.0, h: 12.0 };
                rects.push(UiRect::framed(d, c(slot), 6.0, 1.0, c(Slot::Border2)));
            }
            let ty = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            // Eigene Namen werden nicht uebersetzt (s. push_typed_marker), Presets schon.
            let name = if matches!(wahl, ThemeChoice::Custom(_)) {
                push_typed_marker(texts);
                name
            } else {
                Self::fit_text(gpu, &name, r.w - 26.0 - 64.0, UI_SCALE_SMALL)
            };
            texts.push(UiText { text: name, x: r.x + 26.0, y: ty, color: c(Slot::Text1), scale: UI_SCALE_SMALL });
        }
        // Trennlinie ueber alle sichtbaren Zeilen des Abschnitts.
        let ys: Vec<f32> = (DESIGN_THEME_ROW..design_row_count()).map(settings_row_y).filter(|&y| sichtbar(y)).collect();
        if let (Some(&oben), Some(&unten)) = (ys.first(), ys.last()) {
            rects.push(UiRect { rect: Rect { x: THEME_DIVIDER_X, y: oben - 3.0, w: 1.0, h: unten + SETTINGS_CTRL_H + 3.0 - (oben - 3.0) }, color: g(Slot::Border1), ..UiRect::FLAT });
        }
        // Rechts: welches Theme, dann seine Farben -- bei einem Preset nur zum Ansehen.
        let fest = matches!(set.theme_choice, ThemeChoice::Preset(_));
        if sichtbar(settings_row_y(DESIGN_THEME_ROW)) {
            let name = match set.theme_choice {
                ThemeChoice::Preset(i) => theme::PRESETS.get(i).map_or("", |p| p.name).to_string(),
                ThemeChoice::Custom(i) => set.custom_themes.get(i).map_or_else(String::new, |t| t.name.clone()),
            };
            let y = zeile_y(DESIGN_THEME_ROW);
            let breite = if fest {
                gpu.measure_text(&name, UI_SCALE_SMALL)
            } else {
                push_typed_marker(texts);
                gpu.measure_raw(&name, UI_SCALE_SMALL)
            };
            texts.push(UiText { text: name, x: THEME_COLORS_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
            let hinweis = if fest { "Preset: Farben fest. Eigene Farben mit +" } else { "Farbfeld anklicken zum Ändern, Rechtsklick auf die Kachel löscht" };
            texts.push(UiText { text: hinweis.to_string(), x: THEME_COLORS_X + breite + 16.0, y, color: theme::text_2(), scale: UI_SCALE_SMALL });
        }
        for (i, slot) in theme::Slot::ALL.into_iter().enumerate() {
            let (row, x) = theme_slot_cell(i);
            if !sichtbar(settings_row_y(row)) {
                continue;
            }
            texts.push(UiText { text: slot.label().to_string(), x, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
            // Bei einem Preset gibt es keine Farbfeld-Knoepfe (s. screen_buttons): die Farbe
            // steht hier ohne Hover und ohne Klick.
            if fest {
                let feld = Rect { x: x + THEME_COLOR_SWATCH_DX, y: settings_row_y(row), w: SWATCH_W, h: SETTINGS_CTRL_H };
                rects.push(UiRect::framed(feld, theme::get(slot), 6.0, 2.0, [0.0, 0.0, 0.0, 1.0]));
            }
        }
    }

    // Namensfenster fuer ein neues Theme (Geometrie wie Quick Connect).
    pub(crate) fn build_theme_name_dialog(&self, overlay: &mut UiOverlay) {
        let Some((gpu, d)) = self.gpu.as_ref().zip(self.theme_name_dialog.as_ref()) else { return };
        let alias = d.purpose == NameDialogPurpose::EgoAlias;
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (p, feld, ok, abbrechen) = Self::quick_connect_layout();
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
            UiRect::framed(feld, g(Slot::SurfaceSunken), 6.0, 1.0, g(Slot::Accent)),
        ];
        let mut texts = vec![
            UiText { text: if alias { "Eigener Name" } else { "Neues Theme" }.to_string(), x: p.x + 24.0, y: p.y + 20.0, color: theme::text(), scale: UI_SCALE_NORMAL },
            UiText { text: "NAME".to_string(), x: feld.x, y: feld.y - gpu.text_height(TOPBAR_SUB_SCALE) - 8.0, color: theme::text_3(), scale: TOPBAR_SUB_SCALE },
        ];
        self.push_text_field(gpu, &d.field, feld, true, &mut rects, &mut texts);
        // Ein leerer Alias ist erlaubt: dann bleibt der echte Name.
        let leer = !alias && d.field.text().trim().is_empty();
        for (r, label, primaer) in [(abbrechen, "Abbrechen", false), (ok, if alias { "Übernehmen" } else { "Erstellen" }, true)] {
            let b = Button {
                slider: None,
                action: ButtonAction::NewCustomTheme,
                rect: r,
                label: String::new(),
                enabled: !primaer || !leer,
                color: if primaer { g(Slot::Accent) } else { g(Slot::Surface2) },
                scale: UI_SCALE_SMALL,
            };
            let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, r.contains(cx, cy)));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, label.to_string(), farbe, UI_SCALE_SMALL));
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    pub(crate) fn theme_name_index(&self, x: f32) -> usize {
        let (Some(gpu), Some(d)) = (self.gpu.as_ref(), self.theme_name_dialog.as_ref()) else { return 0 };
        let (_, feld, _, _) = Self::quick_connect_layout();
        text_field::index_at(d.field.text(), x - feld.x - 12.0, |t| gpu.measure_raw(t, UI_SCALE_NORMAL))
    }

    // Linke Taste bei offenem Namensfenster. Immer verbraucht.
    pub(crate) fn theme_name_press(&mut self) {
        let (p, feld, ok, abbrechen) = Self::quick_connect_layout();
        let (cx, cy) = self.cursor_pos;
        if feld.contains(cx, cy) {
            let i = self.theme_name_index(cx);
            let shift = self.modifiers.shift_key();
            if let Some(d) = self.theme_name_dialog.as_mut() {
                if field_press(&mut d.field, &mut d.last_press, &mut d.dragging, i, shift, (cx, cy)) {
                    self.selection_sound_hint = Some(ui_sound::UiSound::Step);
                }
            }
        } else if ok.contains(cx, cy) {
            self.create_custom_theme();
        } else if abbrechen.contains(cx, cy) || !p.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.theme_name_dialog = None;
        }
    }

    // Neues eigenes Theme mit dem Namen aus dem Fenster, anfangs mit den Farben des gerade
    // gewaehlten -- und gleich gewaehlt, damit man rechts die Farben setzen kann.
    pub(crate) fn create_custom_theme(&mut self) {
        if self.theme_name_dialog.as_ref().is_some_and(|d| d.purpose == NameDialogPurpose::EgoAlias) {
            let alias = self.theme_name_dialog.take().map(|d| d.field.text().trim().to_string()).unwrap_or_default();
            self.play_ui_sound(ui_sound::UiSound::Click);
            self.settings.ego_alias = alias;
            self.persist_settings();
            return;
        }
        let name = self.theme_name_dialog.as_ref().map(|d| d.field.text().trim().to_string()).unwrap_or_default();
        if name.is_empty() {
            self.play_ui_sound(ui_sound::UiSound::Denied);
            return;
        }
        self.play_ui_sound(ui_sound::UiSound::Click);
        self.theme_name_dialog = None;
        let colors = self.settings.theme;
        self.settings.custom_themes.push(CustomTheme { name, colors });
        let i = self.settings.custom_themes.len() - 1;
        self.settings.select_theme(ThemeChoice::Custom(i));
        self.persist_settings();
    }

    // Eigenes Theme unter dem Zeiger (Seite Aussehen), Index in custom_themes.
    pub(crate) fn custom_theme_at_pointer(&self) -> Option<usize> {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsDesign {
            return None;
        }
        let (qx, qy) = self.pointer();
        (0..self.settings.custom_themes.len()).find(|&j| theme_tile_rect(theme::PRESETS.len() + j).contains(qx, qy))
    }

    // Disconnect-Fenster: Ueberschrift mittig, darunter die Meldung (umbrochen, jede Zeile
    // mittig), ganz unten mittig "Schliessen". Die Hoehe waechst mit der Meldung.
    // (Fenster, Schliessen, Zeilen)
    pub(crate) fn disconnect_popup_layout(&self) -> Option<(Rect, Rect, Vec<String>)> {
        let (gpu, meldung) = self.gpu.as_ref().zip(self.disconnect_popup.as_ref())?;
        let (sw, sh) = ui_screen();
        let w = 560.0_f32.min(sw - 32.0);
        let zeile_h = gpu.text_height(UI_SCALE_SMALL) + 6.0;
        let titel_h = gpu.text_height(UI_SCALE_NORMAL);
        // Hoechstens so viele Zeilen, wie aufs Fenster passen; der Rest steht im Log.
        let max_zeilen = (((sh - 32.0 - 24.0 - titel_h - 20.0 - 24.0 - 40.0 - 24.0) / zeile_h).floor() as usize).max(1);
        let mut zeilen: Vec<String> = meldung.lines().flat_map(|absatz| wrap_measured(|t| gpu.measure_raw(t, UI_SCALE_SMALL), absatz, w - 48.0)).collect();
        if zeilen.len() > max_zeilen {
            zeilen.truncate(max_zeilen);
            if let Some(letzte) = zeilen.last_mut() {
                letzte.push_str(" …");
            }
        }
        let h = 24.0 + titel_h + 20.0 + zeilen.len() as f32 * zeile_h + 24.0 + 40.0 + 24.0;
        let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
        let schliessen = Rect { x: (p.x + (p.w - 140.0) / 2.0).round(), y: p.y + p.h - 24.0 - 40.0, w: 140.0, h: 40.0 };
        Some((p, schliessen, zeilen))
    }

    pub(crate) fn build_disconnect_popup(&self, overlay: &mut UiOverlay) {
        if self.mode != AppMode::Menu {
            return;
        }
        let (Some(gpu), Some((p, schliessen, zeilen))) = (self.gpu.as_ref(), self.disconnect_popup_layout()) else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
        ];
        let titel_h = gpu.text_height(UI_SCALE_NORMAL);
        let titel = Rect { x: p.x, y: p.y + 24.0, w: p.w, h: titel_h };
        let mut texts = vec![gpu.centered_text(titel, "Disconnected".to_string(), theme::text(), UI_SCALE_NORMAL)];
        // Die Meldung kommt vom Server: nicht uebersetzen, keine Zahlen "lernen".
        push_typed_marker(&mut texts);
        let zeile_h = gpu.text_height(UI_SCALE_SMALL) + 6.0;
        let mut y = titel.y + titel_h + 20.0;
        for z in zeilen {
            let breite = gpu.measure_raw(&z, UI_SCALE_SMALL);
            texts.push(UiText { text: z, x: (p.x + (p.w - breite) / 2.0).round(), y, color: theme::text_2(), scale: UI_SCALE_SMALL });
            y += zeile_h;
        }
        let b = Button {
            slider: None,
            action: ButtonAction::Nothing,
            rect: schliessen,
            label: String::new(),
            enabled: true,
            color: g(Slot::Accent),
            scale: UI_SCALE_SMALL,
        };
        let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, schliessen.contains(cx, cy)));
        rects.push(flaeche);
        texts.push(gpu.centered_text(schliessen, "Schließen".to_string(), farbe, UI_SCALE_SMALL));
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Linke Taste bei offenem Disconnect-Fenster: der Knopf oder ein Klick daneben schliesst es.
    pub(crate) fn disconnect_popup_press(&mut self) {
        let Some((p, schliessen, _)) = self.disconnect_popup_layout() else { return };
        let (cx, cy) = self.cursor_pos;
        if schliessen.contains(cx, cy) || !p.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.disconnect_popup = None;
        }
    }

    // Rueckfrage vor dem Loeschen: (Fenster, Loeschen, Abbrechen).
    pub(crate) fn theme_delete_layout() -> (Rect, Rect, Rect) {
        let (sw, sh) = ui_screen();
        let (w, h) = (440.0, 176.0);
        let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
        let loeschen = Rect { x: p.x + p.w - 24.0 - 140.0, y: p.y + p.h - 24.0 - 40.0, w: 140.0, h: 40.0 };
        let abbrechen = Rect { x: loeschen.x - 12.0 - 140.0, ..loeschen };
        (p, loeschen, abbrechen)
    }

    pub(crate) fn build_theme_delete(&self, overlay: &mut UiOverlay) {
        let Some((gpu, i)) = self.gpu.as_ref().zip(self.theme_delete).filter(|_| self.mode == AppMode::Menu) else { return };
        let Some(t) = self.settings.custom_themes.get(i) else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (p, loeschen, abbrechen) = Self::theme_delete_layout();
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
        ];
        let mut texts = vec![UiText { text: "Löschen?".to_string(), x: p.x + 24.0, y: p.y + 20.0, color: theme::text(), scale: UI_SCALE_NORMAL }];
        push_typed_marker(&mut texts);
        texts.push(UiText { text: t.name.clone(), x: p.x + 24.0, y: p.y + 24.0 + gpu.text_height(UI_SCALE_NORMAL) + 10.0, color: theme::text_2(), scale: UI_SCALE_SMALL });
        for (r, label, gefahr) in [(abbrechen, "Abbrechen", false), (loeschen, "Löschen", true)] {
            let b = Button {
                slider: None,
                action: ButtonAction::Nothing,
                rect: r,
                label: String::new(),
                enabled: true,
                color: if gefahr { g(Slot::DangerSurface) } else { g(Slot::Surface2) },
                scale: UI_SCALE_SMALL,
            };
            let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, r.contains(cx, cy)));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, label.to_string(), farbe, UI_SCALE_SMALL));
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Linke Taste bei offener Rueckfrage. Immer verbraucht.
    pub(crate) fn theme_delete_press(&mut self) {
        let (p, loeschen, abbrechen) = Self::theme_delete_layout();
        let (cx, cy) = self.cursor_pos;
        if loeschen.contains(cx, cy) {
            if let Some(i) = self.theme_delete.take() {
                self.play_ui_sound(ui_sound::UiSound::Close);
                self.delete_custom_theme(i);
            }
        } else if abbrechen.contains(cx, cy) || !p.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.theme_delete = None;
        }
    }

    // Eigenes Theme `i` entfernen; war es gewaehlt, gilt danach der Standard.
    pub(crate) fn delete_custom_theme(&mut self, i: usize) {
        let set = &mut self.settings;
        if i >= set.custom_themes.len() {
            return;
        }
        set.custom_themes.remove(i);
        match set.theme_choice {
            ThemeChoice::Custom(j) if j == i => set.select_theme(ThemeChoice::Preset(0)),
            ThemeChoice::Custom(j) if j > i => set.theme_choice = ThemeChoice::Custom(j - 1),
            _ => {}
        }
        self.persist_settings();
    }

    pub(crate) fn build_settings_language_tab(&self, gpu: &GpuState, texts: &mut Vec<UiText>) {
        let beschriftungen = LANGUAGE_ROWS.iter().map(|(label, _)| *label).chain(["Längezeichen (Latein)", "Lehnwörter auf Englisch"]);
        for (row, label) in beschriftungen.enumerate() {
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        let y = settings_row_y(LOANWORDS_ROW + 1) + 10.0;
        for (i, zeile) in wrap_text(gpu, LANGUAGE_HINT, UI_SCALE_SMALL, gpu.size().0 - SETTINGS_LABEL_X - 40.0).into_iter().enumerate() {
            texts.push(UiText {
                text: zeile,
                x: SETTINGS_LABEL_X,
                y: y + i as f32 * (gpu.text_height(UI_SCALE_SMALL) + 4.0),
                color: theme::text_disabled(),
                scale: UI_SCALE_SMALL,
            });
        }
    }

    // Die Sprache einer Zeile im Sprach-Tab; "Alles" nur, wenn alle Bereiche gleich sind.
    pub(crate) fn language_of_row(&self, row: usize) -> Option<i18n::Language> {
        let set = &self.settings;
        let bereiche = [set.lang_ui, set.lang_items, set.lang_effects, set.lang_chat];
        let index = match row {
            0 => bereiche.iter().all(|&b| b == bereiche[0]).then_some(bereiche[0])?,
            r => *bereiche.get(r - 1)?,
        };
        Some(i18n::Language::from_index(index))
    }

    pub(crate) fn build_settings_font_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let allgemein = [(0, "Standard"), (STABLE_NUMBERS_ROW, "Stabile Zahlen")];
        let ausnahmen = ShadowArea::ALL.into_iter().map(|a| (FONT_EXCEPTIONS_ROW + a.index(), a.label()));
        for (row, text) in allgemein.into_iter().chain(ausnahmen) {
            texts.push(UiText { text: text.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
        }
        // Schattenstaerke neben jedem gefuellten Schatten-Kaestchen (s. editable_shown).
        for ev in EditableValue::ALL.into_iter().filter(|ev| ev.screen() == MenuScreen::SettingsFont && self.editable_shown(*ev)) {
            if ev.rect().y < 0.0 {
                continue;
            }
            let wert = self.editable_number(ev);
            let text = match ev {
                EditableValue::ShadowThickness(_) => format!("{wert:.1} px"),
                _ => format!("{wert:.0} %"),
            };
            self.draw_editable_value(gpu, ev.rect(), ev, text, rects, texts);
        }
    }

    // Die Kaestchen des Schrift-Tabs, in Inhaltskoordinaten. Erst nach fit_texts gezeichnet,
    // damit der Name in seiner eigenen Schrift nicht mit der allgemeinen eingepasst wird.
    pub(crate) fn build_font_slots(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let ziel = self.font_drop_target();
        for slot in FontSlot::all() {
            let Some(r) = slot.rect() else { continue };
            // Was gerade herausgezogen wird, fehlt schon im Kaestchen.
            let heraus = !slot.is_default() && self.font_drag.is_some_and(|d| d.from == Some(slot));
            let inhalt = if heraus { None } else { self.settings.font_slot(slot) };
            let rahmen = if ziel == Some(slot) { theme::get(theme::Slot::Accent) } else { theme::get(theme::Slot::Border1) };
            Self::push_font_slot_tile(gpu, rects, texts, r, inhalt, rahmen);
        }
    }

    // Ein Kaestchen links im Schrift-Tab: leer, oder der Name der Schrift (in ihr gesetzt)
    // bzw. des Schattens.
    pub(crate) fn push_font_slot_tile(gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, r: Rect, tile: Option<FontTile>, rahmen: [f32; 4]) {
        rects.push(UiRect::framed(r, theme::get(theme::Slot::SurfaceSunken), 4.0, 1.0, rahmen));
        let Some(tile) = tile else { return };
        let font = match tile {
            FontTile::Font(kind) => Some(kind),
            FontTile::Shadow(_) => None,
        };
        gpu.set_layout_font(font);
        let name = ellipsize(gpu, tile.label(), UI_SCALE_SMALL, r.w - 12.0);
        let w = gpu.measure_text(&name, UI_SCALE_SMALL);
        let y = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        push_font_marker(texts, font);
        texts.push(UiText { text: name, x: (r.x + (r.w - w) / 2.0).round(), y, color: theme::text(), scale: UI_SCALE_SMALL });
        push_font_marker(texts, None);
        gpu.set_layout_font(None);
    }

    // Eine Kachel der Auswahl rechts: oben die Probe (Schrift: "Gabriel" und darunter Ziffern
    // in ihr; Schatten: ein grosses A damit auf grau-gelben Karos), darunter ein Strich und der
    // Name. Schriften ohne Rahmen, die Schatten stehen in einem Raster (s. font_palette).
    pub(crate) fn push_font_palette_tile(gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, r: Rect, tile: FontTile) {
        let name_h = font_palette_name_h(gpu);
        let probe = Rect { h: r.h - name_h, ..r };
        match tile {
            FontTile::Shadow(stil) => {
                let mut y = 0.0;
                while y < probe.h {
                    let mut x = 0.0;
                    while x < probe.w {
                        let gelb = ((x / SHADOW_CHECKER_PX) as usize + (y / SHADOW_CHECKER_PX) as usize) % 2 == 1;
                        let feld = Rect { x: probe.x + x, y: probe.y + y, w: SHADOW_CHECKER_PX.min(probe.w - x), h: SHADOW_CHECKER_PX.min(probe.h - y) };
                        rects.push(UiRect { rect: feld, color: if gelb { SHADOW_CHECKER_YELLOW } else { SHADOW_CHECKER_GREY }, ..UiRect::FLAT });
                        x += SHADOW_CHECKER_PX;
                    }
                    y += SHADOW_CHECKER_PX;
                }
                let scale = 2.0 * UI_SCALE_SMALL;
                let w = gpu.measure_text("A", scale);
                let y = (probe.y + (probe.h - gpu.text_height(scale)) / 2.0).round();
                push_shadow_marker(texts, Some((stil, 1.0)));
                texts.push(UiText { text: "A".to_string(), x: (probe.x + (probe.w - w) / 2.0).round(), y, color: SHADOW_PREVIEW_TEXT, scale });
                push_shadow_marker(texts, None);
            }
            FontTile::Font(kind) => {
                gpu.set_layout_font(Some(kind));
                let zeile = gpu.text_height(UI_SCALE_SMALL) + 2.0;
                let oben = (probe.y + (probe.h - 2.0 * zeile + 2.0) / 2.0).round();
                push_font_marker(texts, Some(kind));
                for (i, wort) in ["Gabriel", "1247"].into_iter().enumerate() {
                    let w = gpu.measure_text(wort, UI_SCALE_SMALL);
                    texts.push(UiText { text: wort.to_string(), x: (probe.x + (probe.w - w) / 2.0).round(), y: oben + i as f32 * zeile, color: theme::text(), scale: UI_SCALE_SMALL });
                }
                push_font_marker(texts, None);
                gpu.set_layout_font(None);
            }
        }
        rects.push(UiRect { rect: Rect { x: r.x, y: probe.y + probe.h, w: r.w, h: 1.0 }, color: theme::get(theme::Slot::Border1), ..UiRect::FLAT });
        let name = ellipsize(gpu, tile.label(), UI_SCALE_SMALL, r.w - 8.0);
        let w = gpu.measure_text(&name, UI_SCALE_SMALL);
        let y = (probe.y + probe.h + (name_h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: name, x: (r.x + (r.w - w) / 2.0).round(), y, color: theme::text(), scale: UI_SCALE_SMALL });
    }

    // Die Auswahl im Rand rechts des Schrift-Tabs, in Bildschirmkoordinaten: oben die
    // Schriften, vier je Reihe, dann ein Strich quer durch den Rand und darunter die Schatten
    // in einem Raster, ebenfalls vier je Reihe.
    pub(crate) fn font_palette(&self, gpu: &GpuState) -> FontPalette {
        const PADDING: f32 = 12.0;
        const ABSTAND: f32 = 16.0;
        const JE_REIHE: usize = 4;
        let (sw, _) = gpu.size();
        let rand = self.side_strip_w(sw);
        let links = sw - rand + 8.0 + PADDING;
        let rechts = sw - 8.0 - PADDING;
        let th = gpu.text_height(UI_SCALE_SMALL);
        let name_h = font_palette_name_h(gpu);
        let mut y = SETTINGS_FIRST_Y + SETTINGS_CONTENT_DY;
        let mut headings = Vec::new();
        let mut tiles = Vec::new();
        let mut lines = Vec::new();
        // Schriften: Kacheln mit Abstand.
        headings.push(("Schriften", y));
        y += th + 10.0;
        let w = ((rechts - links - (JE_REIHE - 1) as f32 * ABSTAND) / JE_REIHE as f32).floor();
        let h = (2.0 * (th + 2.0) + 20.0 + name_h).round();
        let schriften: Vec<FontTile> = FontTile::palette().filter(|t| matches!(t, FontTile::Font(_))).collect();
        for (i, tile) in schriften.iter().enumerate() {
            let (spalte, reihe) = (i % JE_REIHE, i / JE_REIHE);
            tiles.push((*tile, Rect { x: links + spalte as f32 * (w + ABSTAND), y: y + reihe as f32 * (h + ABSTAND), w, h }));
        }
        y += schriften.len().div_ceil(JE_REIHE) as f32 * (h + ABSTAND) + 4.0;
        // Strich quer durch den Rand.
        lines.push(Rect { x: sw - rand, y: y.round(), w: rand, h: 1.0 });
        y += 20.0;
        headings.push(("Schatten", y));
        y += th + 10.0;
        // Schatten: Zellen ohne Luecke, die Linien dazwischen sind das Raster.
        let w = ((rechts - links) / JE_REIHE as f32).floor();
        let h = (2.0 * gpu.text_height(UI_SCALE_SMALL) + 24.0 + name_h).round();
        let schatten: Vec<FontTile> = FontTile::palette().filter(|t| matches!(t, FontTile::Shadow(_))).collect();
        let reihen = schatten.len().div_ceil(JE_REIHE);
        for (i, tile) in schatten.iter().enumerate() {
            let (spalte, reihe) = (i % JE_REIHE, i / JE_REIHE);
            tiles.push((*tile, Rect { x: links + spalte as f32 * w, y: y + reihe as f32 * h, w, h }));
        }
        let raster = |lines: &mut Vec<Rect>| {
            for reihe in 0..reihen {
                let in_reihe = (schatten.len() - reihe * JE_REIHE).min(JE_REIHE);
                let oben = y + reihe as f32 * h;
                let breite = in_reihe as f32 * w;
                lines.push(Rect { x: links, y: oben, w: breite + 1.0, h: 1.0 });
                lines.push(Rect { x: links, y: oben + h, w: breite + 1.0, h: 1.0 });
                for k in 0..=in_reihe {
                    lines.push(Rect { x: links + k as f32 * w, y: oben, w: 1.0, h: h + 1.0 });
                }
            }
        };
        raster(&mut lines);
        y += reihen as f32 * h + 2.0 * ABSTAND;
        FontPalette { x: links, headings, tiles, lines, bottom: y }
    }

    pub(crate) fn build_font_palette(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let palette = self.font_palette(gpu);
        for (titel, y) in palette.headings {
            texts.push(UiText { text: titel.to_string(), x: palette.x, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        for (tile, r) in palette.tiles {
            Self::push_font_palette_tile(gpu, rects, texts, r, tile);
        }
        for l in palette.lines {
            rects.push(UiRect { rect: l, color: theme::get(theme::Slot::Border1), ..UiRect::FLAT });
        }
    }

    // Breite des Rands rechts auf den Einstellungsseiten (s. tooltip_strip_w). Im Schrift-Tab
    // doppelt so breit: dort steht ueber den Erklaerungen die Auswahl zum Hineinziehen.
    pub(crate) fn side_strip_w(&self, screen_w: f32) -> f32 {
        let w = tooltip_strip_w(screen_w);
        match self.screen {
            MenuScreen::SettingsFont => 2.0 * w,
            // Die Partikel-Tabelle braucht die ganze Breite und hat keine Erklaerungen, ebenso
            // Aussehen (Kacheln und Farben nebeneinander) und die Module (Kacheln, ohne Tooltips).
            MenuScreen::SettingsParticles | MenuScreen::SettingsDesign | MenuScreen::SettingsModules => 0.0,
            _ => w,
        }
    }

    // Kaestchen des Schrift-Tabs unter dem Zeiger (Bildschirmkoordinaten).
    pub(crate) fn font_slot_at(&self, x: f32, y: f32) -> Option<FontSlot> {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsFont {
            return None;
        }
        let (dx, dy) = self.content_offset();
        FontSlot::all().find(|s| s.rect().is_some_and(|r| r.contains(x - dx, y - dy)))
    }

    // Wohin das gezogene Kaestchen fiele: das unter dem Zeiger, wenn es dazu passt.
    pub(crate) fn font_drop_target(&self) -> Option<FontSlot> {
        let d = self.font_drag?;
        self.font_slot_at(self.cursor_pos.0, self.cursor_pos.1).filter(|s| s.accepts(d.tile))
    }

    // Druck im Schrift-Tab: ein Kaestchen der Auswahl oder ein gefuelltes links wird
    // aufgenommen. false, wenn dort keins ist.
    pub(crate) fn font_drag_press(&mut self) -> bool {
        let (cx, cy) = self.cursor_pos;
        let aus_auswahl = self
            .gpu
            .as_ref()
            .filter(|_| self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsFont)
            .and_then(|gpu| self.font_palette(gpu).tiles.into_iter().find(|(_, r)| r.contains(cx, cy)))
            .map(|(tile, r)| (tile, None, r));
        let gefunden = aus_auswahl.or_else(|| {
            let slot = self.font_slot_at(cx, cy)?;
            let (r, (dx, dy)) = (slot.rect()?, self.content_offset());
            Some((self.settings.font_slot(slot)?, Some(slot), Rect { x: r.x + dx, y: r.y + dy, ..r }))
        });
        let Some((tile, from, r)) = gefunden else { return false };
        // Am Zeiger haengt es in der Groesse der Kaestchen links: aus der groesseren Auswahl
        // gegriffen, haelt er es in der Mitte.
        let grab = if from.is_some() { (cx - r.x, cy - r.y) } else { (tile.box_w() / 2.0, FONT_SLOT_H / 2.0) };
        self.font_drag = Some(FontDrag { tile, from, grab });
        true
    }

    // Loslassen: ins passende Kaestchen unter dem Zeiger. Was aus einem Kaestchen kam und
    // woanders landet, ist dort weg -- auch, wenn es neben jedes Kaestchen faellt.
    pub(crate) fn font_drag_release(&mut self) {
        let Some(d) = self.font_drag.take() else { return };
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsFont {
            return;
        }
        let ziel = self.font_slot_at(self.cursor_pos.0, self.cursor_pos.1).filter(|s| s.accepts(d.tile));
        if ziel.is_some() && ziel == d.from {
            return;
        }
        if let Some(ziel) = ziel {
            self.set_font_slot(ziel, Some(d.tile));
            self.play_ui_sound(ui_sound::UiSound::Click);
        }
        if let Some(von) = d.from {
            self.set_font_slot(von, None);
        }
        self.persist_settings();
    }

    // Ein Kaestchen des Schrift-Tabs fuellen oder leeren (None). Standard-Schrift und
    // -Schatten lassen sich nicht leeren.
    pub(crate) fn set_font_slot(&mut self, slot: FontSlot, tile: Option<FontTile>) {
        let schrift = match (slot, tile) {
            (FontSlot::DefaultFont, Some(FontTile::Font(kind))) => {
                let i = area_font_code(Some(kind)) - 1;
                if self.settings.font == i {
                    return;
                }
                self.settings.font = i;
                if let Some(gpu) = self.gpu.as_mut() {
                    if let Err(e) = gpu.font_atlas.switch(&gpu.device, &gpu.queue, &mut self.installation, kind) {
                        eprintln!("Schrift konnte nicht gewechselt werden: {e}");
                    }
                }
                true
            }
            (FontSlot::DefaultShadow, Some(FontTile::Shadow(stil))) => {
                self.settings.set_global_text_shadow(stil);
                self.sync_text_shadow();
                false
            }
            (FontSlot::AreaFont(a), t) => {
                let font = match t {
                    Some(FontTile::Font(kind)) => Some(kind),
                    _ => None,
                };
                self.settings.area_font[a.index()] = area_font_code(font);
                true
            }
            (FontSlot::AreaShadow(a), t) => {
                self.settings.area_shadow[a.index()] = match t {
                    Some(FontTile::Shadow(stil)) => area_shadow_code(stil),
                    _ => 0,
                };
                false
            }
            _ => return,
        };
        // Andere Schrift, andere Breiten -- die eingerasteten gelten nicht mehr.
        if schrift {
            self.hud_latched_widths.borrow_mut().clear();
            if let Some(gpu) = self.gpu.as_ref() {
                gpu.stable_numbers.borrow_mut().clear();
            }
        }
    }

    // Sens-Rechner: Eingaben (anklickbar, s. EditableValue::CalcMinecraftSens/CalcDpi) und
    // die Ergebnisse als reine Anzeige. "Uebernehmen" kommt aus buttons().
    pub(crate) fn build_sens_calc(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let set = &self.settings;
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let label = |row: usize, text: &str, texts: &mut Vec<UiText>| {
            texts.push(UiText { text: text.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
        };
        let anzeige = |row: usize, text: String, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>| {
            let rect = stepper_value_rect(row);
            rects.push(UiRect { rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
            texts.push(gpu.centered_text(rect, text, theme::text(), UI_SCALE_SMALL));
        };
        let umgerechnet = minecraft_sens_to_client(set.sens_calc_mc_percent);
        let geklemmt = umgerechnet.clamp(SENSITIVITY_MIN, SENSITIVITY_MAX);

        label(SENS_CALC_MC_ROW, "Minecraft-Sens", texts);
        self.draw_editable_value(
            gpu,
            stepper_value_rect(SENS_CALC_MC_ROW),
            EditableValue::CalcMinecraftSens,
            format!("{} %", trimme_nullen(format!("{:.2}", set.sens_calc_mc_percent))),
            rects,
            texts,
        );
        label(SENS_CALC_RESULT_ROW, "= Coral-Client-Sens", texts);
        anzeige(SENS_CALC_RESULT_ROW, format_sensitivity(umgerechnet), rects, texts);
        if (geklemmt - umgerechnet).abs() > f32::EPSILON {
            texts.push(UiText {
                text: format!("außerhalb des Bereichs -- wird auf {} begrenzt", format_sensitivity(geklemmt)),
                x: SETTINGS_LABEL_X,
                y: zeile_y(SENS_CALC_RESULT_ROW + 1),
                color: theme::text_disabled(),
                scale: UI_SCALE_SMALL,
            });
        }

        label(SENS_CALC_DPI_ROW, "DPI der Maus", texts);
        self.draw_editable_value(gpu, stepper_value_rect(SENS_CALC_DPI_ROW), EditableValue::CalcDpi, set.sens_calc_dpi.to_string(), rects, texts);
        let cm = |sens: f32| format!("{:.1} cm", cm_per_360(sens, set.sens_calc_dpi));
        label(SENS_CALC_CM_MC_ROW, "cm/360 mit der Minecraft-Sens", texts);
        anzeige(SENS_CALC_CM_MC_ROW, cm(umgerechnet), rects, texts);
        label(SENS_CALC_CM_OWN_ROW, &format!("cm/360 mit Coral-Client-Sens {}", format_sensitivity(set.mouse_sensitivity)), texts);
        anzeige(SENS_CALC_CM_OWN_ROW, cm(set.mouse_sensitivity), rects, texts);
        texts.push(UiText {
            text: "Werte anklicken zum Eintippen. Gilt für die Maus ohne Beschleunigung (Rohdaten), wie in Minecraft.".to_string(),
            x: SETTINGS_LABEL_X,
            y: zeile_y(SENS_CALC_CM_OWN_ROW + 2),
            color: theme::text_disabled(),
            scale: UI_SCALE_SMALL,
        });
    }

    // Video-Tab: Glint-Regler links unter den Partikeln, Crit-Farbe rechts unter der
    // Schadens-Faerbung.
    pub(crate) fn build_video_glint_and_crit(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        for (row, label, feld, wert) in [
            (GLINT_SPEED_ROW, "Glint-Geschwindigkeit", EditableValue::GlintSpeed, self.settings.glint_speed),
            (GLINT_STRENGTH_ROW, "Glint-Stärke", EditableValue::GlintStrength, self.settings.glint_strength),
            (PLAYER_LIST_SCALE_ROW, "Größe", EditableValue::PlayerListScale, self.settings.player_list_scale),
        ] {
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
            self.draw_editable_value(gpu, stepper_value_rect(row), feld, format!("{wert} %"), rects, texts);
        }

        texts.push(UiText { text: "Eigene Crit-Farbe".to_string(), x: SETTINGS_LABEL_X, y: zeile_y(CRIT_COLOR_ROW), color: theme::text(), scale: UI_SCALE_SMALL });

        let set = &self.settings;
        let detail = if set.fire_mode == FIRE_MODE_CROSSHAIR { "Farbe" } else { "Höhe" };
        for (row, label) in [(HURT_CAM_ROW, "Hurt-Cam"), (HURT_CAM_STRENGTH_ROW, "Stärke"), (FIRE_MODE_ROW, "Modus"), (FIRE_DETAIL_ROW, detail), (DEATH_ANIM_ROW, "Todesanimation")] {
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
        }
        let mut werte = vec![(HURT_CAM_STRENGTH_ROW, set.hurt_cam_strength)];
        if set.fire_mode != FIRE_MODE_CROSSHAIR {
            werte.push((FIRE_DETAIL_ROW, set.fire_height));
        }
        for (row, wert) in werte {
            let value_rect = stepper_value_rect(row);
            rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(value_rect, format!("{wert} %"), theme::text(), UI_SCALE_SMALL));
        }
    }

    pub(crate) fn build_settings_configs(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: format!("Aktiv: {}", active_config()), x: SETTINGS_LABEL_X, y: zeile(0), color: theme::text(), scale: UI_SCALE_SMALL });
        if let Some((InputKind::ConfigName(edit), wert)) = &self.input_field {
            let label = match edit {
                ConfigEdit::New => "Name der neuen Config",
                ConfigEdit::Duplicate => "Name der Kopie",
                ConfigEdit::Rename => "Neuer Name",
            };
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y: zeile(1), color: theme::text(), scale: UI_SCALE_SMALL });
            let feld = config_name_field_rect();
            rects.push(UiRect { rect: feld, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            let tx = feld.x + 10.0;
            push_typed_marker(texts);
            texts.push(UiText { text: wert.clone(), x: tx, y: zeile(1), color: theme::text(), scale: UI_SCALE_SMALL });
            self.push_caret(rects, Rect { x: tx + gpu.measure_raw(wert, UI_SCALE_SMALL), y: zeile(1), w: CARET_WIDTH_PX, h: gpu.text_height(UI_SCALE_SMALL) }, COLOR_CARET);
        } else {
            texts.push(UiText {
                text: "Klick auf eine Config wechselt sofort -- auch mitten im Spiel.".to_string(),
                x: SETTINGS_LABEL_X,
                y: zeile(1),
                color: theme::text_disabled(),
                scale: UI_SCALE_SMALL,
            });
        }
        texts.push(UiText { text: "Configs".to_string(), x: SETTINGS_LABEL_X, y: zeile(CONFIG_LIST_FIRST_ROW), color: theme::text(), scale: UI_SCALE_SMALL });
        let anzahl = assets::list_configs().len();
        if anzahl > MAX_VISIBLE_CONFIGS {
            texts.push(UiText {
                text: format!("+ {} weitere (im Launcher)", anzahl - MAX_VISIBLE_CONFIGS),
                x: SETTINGS_CTRL_X,
                y: zeile(CONFIG_LIST_FIRST_ROW + MAX_VISIBLE_CONFIGS),
                color: theme::text_disabled(),
                scale: UI_SCALE_SMALL,
            });
        }
    }

    pub(crate) fn build_settings_light_levels(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        // Suchfeld: Tippen geht immer hierhin, solange die Seite offen ist.
        texts.push(UiText { text: "Suche".to_string(), x: SETTINGS_LABEL_X, y: zeile(0), color: theme::text(), scale: UI_SCALE_SMALL });
        let feld = Rect { x: SETTINGS_CTRL_X, y: settings_row_y(0), w: STEPPER_VALUE_W + 2.0 * (STEPPER_BTN_W + STEPPER_GAP), h: SETTINGS_CTRL_H };
        rects.push(UiRect { rect: feld, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        let tx = feld.x + 10.0;
        let (shown, farbe) = if self.light_search.is_empty() { ("Fackel, Laterne, ...".to_string(), theme::text_disabled()) } else { (self.light_search.clone(), theme::text()) };
        let text_x = if self.light_search.is_empty() { tx + CARET_WIDTH_PX + 3.0 } else { tx };
        push_typed_marker(texts);
        texts.push(UiText { text: shown, x: text_x, y: zeile(0), color: farbe, scale: UI_SCALE_SMALL });
        let caret_x = tx + if self.light_search.is_empty() { 0.0 } else { gpu.measure_raw(&self.light_search, UI_SCALE_SMALL) };
        self.push_caret(rects, Rect { x: caret_x, y: zeile(0), w: CARET_WIDTH_PX, h: gpu.text_height(UI_SCALE_SMALL) }, COLOR_CARET);
        let treffer = self.light_search_hits();
        if treffer.is_empty() {
            texts.push(UiText { text: "Keine Lichtquelle gefunden.".to_string(), x: SETTINGS_LABEL_X, y: zeile(1), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
        }
        let sichtbar = self.light_visible_rows();
        for (z, &i) in treffer.iter().skip(self.light_scroll).take(sichtbar).enumerate() {
            let id = light_source_blocks()[i];
            let p = self.settings.light_factor_of(id);
            let farbe = if p == 100 { theme::text() } else { COLOR_HUD };
            texts.push(UiText { text: block_display_name(id), x: SETTINGS_LABEL_X, y: zeile(1 + z), color: farbe, scale: UI_SCALE_SMALL });
            let rect = stepper_value_rect(1 + z);
            rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(rect, format!("{p} %"), farbe, UI_SCALE_SMALL));
        }
        if treffer.len() > sichtbar {
            let bis = (self.light_scroll + sichtbar).min(treffer.len());
            texts.push(UiText {
                text: format!("{}–{} von {} (Mausrad)", self.light_scroll + 1, bis, treffer.len()),
                x: SETTINGS_CTRL_X + STEPPER_VALUE_W + 2.0 * (STEPPER_BTN_W + STEPPER_GAP) + 20.0,
                y: zeile(0),
                color: theme::text_disabled(),
                scale: UI_SCALE_SMALL,
            });
        }
    }

    pub(crate) fn build_settings_particles(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        // Spaltenkoepfe ueber den Knoepfen. Die vierte Spalte ist je nach Art Menge (Schwall)
        // oder Intervall (verweilend), s. particles::SpawnKind.
        let regler = |col: usize| (particle_column_x(col), PARTICLE_SLIDER_W);
        let koepfe = [
            ((PARTICLE_TOGGLE_X, PARTICLE_TOGGLE_W), "Zeigen"),
            (regler(0), "Dauer"),
            (regler(1), "Deckkraft"),
            (regler(2), "Tempo"),
            (regler(3), "Menge / Intervall"),
            ((particle_column_x(PARTICLE_ALL_AT_ONCE_COL), PARTICLE_TOGGLE_W), "Auf einmal"),
        ];
        let sichtbar = self.particle_visible_rows();
        let kopf = if particles::KINDS.len() > sichtbar {
            let bis = (self.particle_scroll + sichtbar).min(particles::KINDS.len());
            format!("Partikelart ({}–{} von {})", self.particle_scroll + 1, bis, particles::KINDS.len())
        } else {
            "Partikelart".to_string()
        };
        texts.push(UiText { text: kopf, x: SETTINGS_LABEL_X, y: zeile(0), color: theme::text_2(), scale: UI_SCALE_SMALL });
        for ((x, w), text) in koepfe {
            texts.push(gpu.centered_text(Rect { x, y: settings_row_y(0), w, h: SETTINGS_CTRL_H }, text.to_string(), theme::text_2(), UI_SCALE_SMALL));
        }
        for (z, i) in (0..particles::KINDS.len()).skip(self.particle_scroll).take(sichtbar).enumerate() {
            let t = self.settings.particle_tuning[i];
            let art = particles::KINDS[i].spawn;
            let row = 1 + z;
            let farbe = if t == particles::ParticleTuning::default() { theme::text() } else { COLOR_HUD };
            texts.push(UiText { text: particles::KINDS[i].label.to_string(), x: SETTINGS_LABEL_X, y: zeile(row), color: farbe, scale: UI_SCALE_SMALL });
            let mut werte = vec![t.lifetime_pct, t.opacity_pct, t.speed_pct];
            if art.has_amount() {
                werte.push(t.amount_pct);
            } else if art.has_interval() {
                werte.push(t.interval_pct);
            }
            // Wertfelder wie bei einem Stepper -- der Regler holt sich Flaeche und Zahl daraus.
            for (col, wert) in werte.into_iter().enumerate() {
                let rect = particle_value_rect(col, row);
                rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                let wert_farbe = if wert == 100 { theme::text() } else { COLOR_HUD };
                texts.push(gpu.centered_text(rect, format!("{wert} %"), wert_farbe, UI_SCALE_SMALL));
            }
        }
    }

    pub(crate) fn build_settings_shader(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let y = (settings_row_y(0) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Lichtstreuung".to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        let rect = stepper_value_rect(0);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, format!("{} %", self.settings.shader_scattering_percent), theme::text(), UI_SCALE_SMALL));
        let y1 = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Schattenweichheit".to_string(), x: SETTINGS_LABEL_X, y: y1, color: theme::text(), scale: UI_SCALE_SMALL });
        let rect1 = stepper_value_rect(1);
        rects.push(UiRect { rect: rect1, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        let g = self.settings.shader_sun_size_tenths;
        texts.push(gpu.centered_text(rect1, format!("{}.{}°", g / 10, g % 10), theme::text(), UI_SCALE_SMALL));
        let y2 = (settings_row_y(2) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Blocklicht-Schatten".to_string(), x: SETTINGS_LABEL_X, y: y2, color: theme::text(), scale: UI_SCALE_SMALL });
        // Weichheit nur, solange die Blocklicht-Schatten an sind (sonst ist der Regler aus).
        if self.settings.shader_block_shadows {
            let y3 = (settings_row_y(3) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: "Blocklicht-Weichheit".to_string(), x: SETTINGS_LABEL_X, y: y3, color: theme::text(), scale: UI_SCALE_SMALL });
            let rect3 = stepper_value_rect(3);
            rects.push(UiRect { rect: rect3, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(rect3, format!("{}°", self.settings.shader_block_softness_deg), theme::text(), UI_SCALE_SMALL));
        }
        let y4 = (settings_row_y(4) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let geaendert = self.settings.light_factors.len();
        let text = if geaendert == 0 { "Lichtstärken".to_string() } else { format!("Lichtstärken ({geaendert} geändert)") };
        texts.push(UiText { text, x: SETTINGS_LABEL_X, y: y4, color: theme::text(), scale: UI_SCALE_SMALL });
        let y5 = (settings_row_y(5) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Lichtkarten anzeigen".to_string(), x: SETTINGS_LABEL_X, y: y5, color: theme::text(), scale: UI_SCALE_SMALL });
        let rect5 = stepper_value_rect(5);
        rects.push(UiRect { rect: rect5, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect5, LIGHTMAP_DEBUG_NAMES[self.settings.shader_lightmap_debug as usize].to_string(), theme::text(), UI_SCALE_SMALL));
    }

    pub(crate) fn build_settings_inventory_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let y = (settings_row_y(0) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText {
            // "Opakheit", nicht "Transparenz": der Prozentwert IST die Abdunklung, er laeuft
            // also andersherum als der alte Name sagte (0 % = Welt voll sichtbar, 100 % =
            // deckend schwarz, s. Settings::inventory_transparency). Genau diese Umkehrung war
            // hier schon einmal ein echter Bug im Code -- der Name hat ihn nahegelegt.
            text: "Hintergrund-Opakheit".to_string(),
            x: SETTINGS_LABEL_X,
            y,
            color: theme::text(),
            scale: UI_SCALE_SMALL,
        });
        let value = format!("{} %", self.settings.inventory_transparency);
        self.draw_editable_value(gpu, stepper_value_rect(0), EditableValue::InventoryTransparency, value, rects, texts);

        let y1 = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Skalierung".to_string(), x: SETTINGS_LABEL_X, y: y1, color: theme::text(), scale: UI_SCALE_SMALL });
        let scale_rect = stepper_value_rect(1);
        rects.push(UiRect { rect: scale_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(scale_rect, Self::scale_label(self.settings.inventory_scale, gpu.size()), theme::text(), UI_SCALE_SMALL));

        let y2 = (settings_row_y(2) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Zeiger in Slotmitte".to_string(), x: SETTINGS_LABEL_X, y: y2, color: theme::text(), scale: UI_SCALE_SMALL });

        let y3 = (settings_row_y(3) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Operator-Items (Kreativ)".to_string(), x: SETTINGS_LABEL_X, y: y3, color: theme::text(), scale: UI_SCALE_SMALL });

        for (zeile, text) in [
            (4, "Zusammenfassen bei Hover"),
            (5, "Trank-Buchstabe S (verstärkt)"),
            (6, "Trank-Buchstabe L (verlängert)"),
            (7, "Trank-Buchstabe W (schwach)"),
        ] {
            let y = (settings_row_y(zeile) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: text.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
    }

    pub(crate) fn build_settings_audio_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let y0 = (settings_row_y(AUDIO_DEVICE_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Ausgabegerät".to_string(), x: SETTINGS_LABEL_X, y: y0, color: theme::text(), scale: UI_SCALE_SMALL });
        // Das Geraet selbst ist eine Aufklappliste aus buttons().

        let y1 = (settings_row_y(AUDIO_TEST_TONE_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Testton".to_string(), x: SETTINGS_LABEL_X, y: y1, color: theme::text(), scale: UI_SCALE_SMALL });

        // Master-Tonhoehe (echt, s. Settings::ui_pitch_semitones) -- Pfeile kommen aus
        // buttons(), hier nur Beschriftung und Wert.
        let y2 = (settings_row_y(AUDIO_PITCH_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText {
            text: "Tonhöhe (Aufnahmen)".to_string(),
            x: SETTINGS_LABEL_X,
            y: y2,
            color: theme::text(),
            scale: UI_SCALE_SMALL,
        });
        let pitch_rect = stepper_value_rect(AUDIO_PITCH_ROW);
        rects.push(UiRect { rect: pitch_rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
        texts.push(gpu.centered_text(pitch_rect, format_ui_pitch(self.settings.ui_pitch_semitones), theme::text(), UI_SCALE_SMALL));

        // Puffer: die Auswahl ist eine Aufklappliste aus buttons(), darunter nur Anzeige, womit
        // der Stream gerade laeuft (s. format_audio_buffer).
        let y5 = (settings_row_y(AUDIO_BUFFER_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Puffer".to_string(), x: SETTINGS_LABEL_X, y: y5, color: theme::text(), scale: UI_SCALE_SMALL });
        let y6 = (settings_row_y(AUDIO_BUFFER_INFO_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Puffer aktiv".to_string(), x: SETTINGS_LABEL_X, y: y6, color: theme::text(), scale: UI_SCALE_SMALL });
        let buffer_rect = settings_wide_rect(AUDIO_BUFFER_INFO_ROW);
        rects.push(UiRect { rect: buffer_rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
        texts.push(gpu.centered_text(buffer_rect, format_audio_buffer(self.audio.buffer_info()), theme::text(), UI_SCALE_SMALL));

        // PipeWire: gemessene Blockgroesse, darunter die Grenze. Deren Knopf kommt aus buttons(),
        // wenn es etwas zu tun gibt; sonst steht hier, warum nicht.
        let y7 = (settings_row_y(AUDIO_PIPEWIRE_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "PipeWire aktiv".to_string(), x: SETTINGS_LABEL_X, y: y7, color: theme::text(), scale: UI_SCALE_SMALL });
        let pw_rect = settings_wide_rect(AUDIO_PIPEWIRE_ROW);
        rects.push(UiRect { rect: pw_rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
        texts.push(gpu.centered_text(pw_rect, self.pipewire_text(), theme::text(), UI_SCALE_SMALL));
        let y8 = (settings_row_y(AUDIO_PIPEWIRE_SET_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "PipeWire-Grenze".to_string(), x: SETTINGS_LABEL_X, y: y8, color: theme::text(), scale: UI_SCALE_SMALL });
        if let PipewireKnopf::Text(text) = self.pipewire_knopf() {
            let rect = settings_wide_rect(AUDIO_PIPEWIRE_SET_ROW);
            rects.push(UiRect { rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
            texts.push(gpu.centered_text(rect, text, theme::text(), UI_SCALE_SMALL));
        }

        // Raumklang-Hoertest (echt, s. spatial_sound::OrbitTest) -- der Knopf selbst kommt
        // aus buttons(), hier nur die Beschriftung davor.
        let y3 = (settings_row_y(AUDIO_SPATIAL_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Raumklang-Test".to_string(), x: SETTINGS_LABEL_X, y: y3, color: theme::text(), scale: UI_SCALE_SMALL });

        // Hauptlautstaerke (echt, s. Settings::master_volume) -- Pfeile aus buttons().
        let y4 = (settings_row_y(AUDIO_MASTER_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Hauptlautstärke".to_string(), x: SETTINGS_LABEL_X, y: y4, color: theme::text(), scale: UI_SCALE_SMALL });
        let volume_label = volume_text(self.settings.master_volume);
        self.draw_editable_value(gpu, stepper_value_rect(AUDIO_MASTER_ROW), EditableValue::MasterVolume, volume_label, rects, texts);

        // Je Kategorie ein Regler (s. VOLUME_CATEGORIES), Pfeile aus buttons().
        for (i, (kategorie, wert)) in VOLUME_CATEGORIES.iter().zip(self.settings.category_volume).enumerate() {
            let row = AUDIO_CATEGORY_FIRST_ROW + i;
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: kategorie.label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
            self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::CategoryVolume(i), volume_text(wert), rects, texts);
        }
        // Je UI-Klang ein Regler (s. ui_sound::UiSound::ALL), Pfeile aus buttons().
        for (i, (klang, wert)) in ui_sound::UiSound::ALL.iter().zip(self.settings.ui_sound_volume).enumerate() {
            let row = AUDIO_UI_SOUND_FIRST_ROW + i;
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: klang.label().to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
            self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::UiSoundVolume(i), volume_text(wert), rects, texts);
        }
    }

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
        for (_, _, name, karte) in self.skin_tiles() {
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
                    sprites.push(UiSprite::quad(b.x, b.y, b.w, b.h, uv, [1.0; 4], SpriteAtlas::PackIcons));
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
        // Gezogene Kachel: Linie in der Luecke, in der sie landen wuerde.
        if let Some((c, platz)) = self.skin_drop_slot() {
            let stand = self.skin_scroll_anim[usize::from(c == Column::Selected)];
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
            let griff = Rect { y: spur.y + (spur.h - griff_h) * (self.skin_scroll_anim[k] / max as f32).clamp(0.0, 1.0), h: griff_h, ..spur };
            rects.push(UiRect::rounded(griff, g(Slot::Border2), 2.0));
        }
    }

    // Die beiden Spalten: links verfuegbar (s. skin_tab::available_column), rechts der Stapel.
    pub(crate) fn skin_columns(&self) -> (Vec<String>, Vec<String>) {
        (skin_tab::available_column(&self.skin_catalog.names, &self.settings.skin_order, &self.selected_skins), self.selected_skins.clone())
    }

    // Erste sichtbare Kachel einer Spalte mit `n` Plaetzen (Scrollstand, aufs Ende geklemmt).
    pub(crate) fn skin_first(&self, c: skin_tab::Column, n: usize) -> usize {
        let scroll = match c {
            skin_tab::Column::Available => self.skin_scroll,
            skin_tab::Column::Selected => self.skin_scroll_right,
        };
        scroll.min(n.saturating_sub(skin_tiles_visible()))
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
            let stand = self.skin_scroll_anim[k];
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
        let stand = self.skin_scroll_anim[usize::from(c == Column::Selected)];
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
                    self.skin_scroll_anim[i] = self.skin_first(c, self.skin_column_len(c)) as f32;
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
        self.skin_files = Some(SkinFilesWindow { title: skin.clone(), skin, mirror: None, entries, expanded: std::collections::HashSet::new(), scroll: 0 });
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
        self.skin_files = Some(SkinFilesWindow { skin, mirror, title, entries, expanded: std::collections::HashSet::new(), scroll: 0 });
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
        let n = skin_files_rows_visible();
        let erste = w.scroll.min(zeilen.len().saturating_sub(n));
        zeilen
            .into_iter()
            .skip(erste)
            .take(n)
            .enumerate()
            .map(|(k, i)| (i, Rect { x: liste.x, y: liste.y + k as f32 * SKIN_FILES_ROW_H, w: liste.w, h: SKIN_FILES_ROW_H }))
            .collect()
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

    pub(crate) fn scroll_skin_files(&mut self, zeilen: f32) {
        let Some(w) = self.skin_files.as_mut() else { return };
        let n = skin_tab::visible_rows(&w.entries, &w.expanded).len();
        let max = n.saturating_sub(skin_files_rows_visible());
        let neu = w.scroll.min(max) as f32 - (zeilen * 3.0).round();
        w.scroll = (neu.max(0.0) as usize).min(max);
    }

    // Mausrad im Skin-Tab: die Spalte unter dem Zeiger.
    pub(crate) fn scroll_skin_column(&mut self, zeilen: f32) {
        use skin_tab::Column;
        let (x, y) = self.pointer();
        let Some(c) = [Column::Available, Column::Selected].into_iter().find(|&c| skin_column_rect(c).contains(x, y)) else { return };
        let (links, rechts) = self.skin_columns();
        let n = match c {
            Column::Available => links.len(),
            Column::Selected => rechts.len(),
        };
        let max = n.saturating_sub(skin_tiles_visible());
        let stand = match c {
            Column::Available => &mut self.skin_scroll,
            Column::Selected => &mut self.skin_scroll_right,
        };
        let neu = (*stand).min(max) as f32 - zeilen.signum() * zeilen.abs().ceil();
        *stand = (neu.max(0.0) as usize).min(max);
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
