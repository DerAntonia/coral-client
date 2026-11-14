// Die Einstellungen und ihre Dateipfade.

use crate::*;

// Zur Laufzeit aenderbare Einstellungen. Jeder Wert hier stand vorher als Konstante im
// Code -- er gehoert aber dem Nutzer, nicht dem Compiler.
pub(crate) struct Settings {
    pub(crate) show_tearing_test: bool,
    pub(crate) show_hud: bool,
    // FPS-Deckel in Hz, mit den Sonderwerten FPS_LIMIT_MONITOR/FPS_LIMIT_UNLIMITED.
    pub(crate) fps_limit: u32,
    // Radiant pro Pixel Maus-Delta.
    pub(crate) mouse_sensitivity: f32,
    // Getrennte Empfindlichkeit fuer die Hoch-/Runter-Bewegung: mouse_sensitivity gilt dann nur
    // noch fuer X, mouse_sensitivity_y fuer Y.
    pub(crate) mouse_sens_split: bool,
    pub(crate) mouse_sensitivity_y: f32,
    // Faktor auf die Zeigerbewegung in Menues/Overlays, s. DEFAULT_CURSOR_SENSITIVITY.
    pub(crate) cursor_sensitivity: f32,
    // Groesse des Mauszeigers in Prozent (s. cursor_fx_frame).
    pub(crate) cursor_size: u32,
    pub(crate) cursor_width: u32,
    // Dicke der Zeiger-Kontur in Prozent, 100 = wie in osu (s. CURSOR_OUTLINE).
    pub(crate) cursor_outline: u32,
    // Durchsichtigkeit des schwarzen Inneren in Prozent, 50 = wie in osu.
    pub(crate) cursor_fill_transparency: u32,
    // Bestaetigungsdauer (wie osus Hold-to-confirm) in ms, und die der Reconnect-Taste;
    // RECONNECT_HOLD_GENERAL dort = die allgemeine.
    pub(crate) hold_confirm_ms: u32,
    pub(crate) reconnect_hold_ms: i32,
    // Kreis unter dem Pfeil des Mauszeigers (s. cursor_polygons).
    pub(crate) cursor_circle: bool,
    // War frueher der Deckel fuer Mesh-Job-VERGABE pro Frame (Job in den Pool einreihen) --
    // das begrenzte den GESAMTEN Durchsatz auf Deckel*FPS ohne echten Nutzen (Einreihen ist
    // billig, s. App::mesh_pending_columns-Kommentar) und wurde deshalb dort entfernt. Jetzt
    // stattdessen der Deckel fuer die ABHOL-Seite: wie viele FERTIGE Mesh-Ergebnisse pro
    // Frame vom Pool abgeholt und als GPU-Buffer hochgeladen werden. DAS ist tatsaechlich
    // teuer genug, um in einem Frame sichtbar zu werden (viele Uploads auf einen Schlag, z.B.
    // wenn nach dem Verbinden ploetzlich hunderte Jobs gleichzeitig fertig werden) -- ohne
    // Deckel sprang deshalb die Tearing-Test-Linie sichtbar, wenn viele Chunks laden. Selber Name/Settings-Slot wie zuvor, nur die Bedeutung hat gewechselt.
    pub(crate) mesh_columns_per_frame: usize,
    // Client-seitige Render-Limiter, s. MAX_RENDER_DISTANCE_MIN/MAX-Kommentar und
    // App::enforce_render_limits -- unabhaengig von protocol::REQUESTED_VIEW_DISTANCE.
    pub(crate) max_render_distance: u32,
    // Worker-Anzahl des dedizierten Mesh-Pools, s. mesh_pool::MeshPool.
    pub(crate) chunk_update_threads: usize,
    // CPU-Render-Ahead-Limit -- wie viele Frames die CPU maximal vorproduzieren darf, bevor
    // sie auf die GPU/Anzeige wartet. Wirkt UNABHAENGIG vom Present-Modus (auch bei
    // Immediate). 1..4 steuern wgpu's desired_maximum_frame_latency (Swapchain-Parameter,
    // nur ein Hinweis an den Treiber). 0 ist ein Sonderfall -- Sodiums "CPU Render-Ahead
    // Limit: 0 frame(s)" nachgebaut per echter Fence-Wait im Renderer selbst (s.
    // GpuState::fence_wait_zero/FRAME_LATENCY_MIN-Kommentar), nicht ueber den (dafuer laut
    // wgpu-Doku ohnehin ungeeigneten) Swapchain-Parameter.
    pub(crate) frame_latency: u32,
    pub(crate) keybinds: Keybinds,
    // Halten/Umschalten je Tastenbelegungs-Aktion, s. KeybindMode. Getrennt von Keybinds
    // (welche physische Taste), damit ein Rebind den gewaehlten Modus nicht mit ueberschreibt.
    pub(crate) keybind_modes: KeybindModes,
    // HUD-Layout: normierte Position (0..1) der linken oberen Ecke des Performance-
    // Elements, im HUD-Editor per Drag gesetzt. Normiert statt Pixel, damit die Position
    // bei anderer Aufloesung/Fenstergroesse gleich sitzt. Weitere HUD-Elemente (Leben,
    // Hunger, Hotbar) bekommen spaeter eigene Felder nach demselben Muster.
    pub(crate) hud_perf_x: f32,
    pub(crate) hud_perf_y: f32,
    // Server-HUD-Positionen (obere linke Ecke, normiert 0..1 wie hud_perf_x/y). Actionbar
    // standardmaessig unten mittig, Scoreboard rechts.
    pub(crate) hud_actionbar_x: f32,
    pub(crate) hud_actionbar_y: f32,
    pub(crate) hud_scoreboard_x: f32,
    pub(crate) hud_scoreboard_y: f32,
    // Feste Ecke der Seitenleiste (SCOREBOARD_ALIGNS): hud_scoreboard_x/y ist diese Ecke, die
    // Leiste waechst von ihr weg. 0 = oben links (wie frueher).
    pub(crate) scoreboard_align: u8,
    // Schriftgroesse der Seitenleiste in Prozent (HUD-Editor).
    pub(crate) hud_scoreboard_scale: u32,
    // Groesse der Pfeil-Anzeige in Prozent eines Hotbar-Slots (HUD-Editor).
    pub(crate) hud_arrows_scale: u32,
    // Itemname ueber der Hotbar: Mitte und Oberkante normiert; y < 0 heisst "an der Hotbar wie
    // in Vanilla" (Standard, s. hud_held_name_rect). Groesse in Prozent der Hotbar-Schrift.
    pub(crate) hud_held_name_x: f32,
    pub(crate) hud_held_name_y: f32,
    pub(crate) hud_held_name_scale: u32,
    // Titel + Untertitel: Ankerpunkt normiert (x je nach Ausrichtung linke Kante, Mitte oder
    // rechte Kante, y Oberkante). Ausrichtung 0 links, 1 mittig, 2 rechts -- wie in Word:
    // der Text waechst vom Anker aus nach rechts, nach beiden Seiten oder nach links.
    pub(crate) hud_title_x: f32,
    pub(crate) hud_title_y: f32,
    pub(crate) hud_title_align: u8,
    // Groesse in Prozent der Vorgabe (UI_SCALE_TITLE bzw. UI_SCALE_NORMAL).
    pub(crate) hud_title_scale: u32,
    // Der Untertitel hat eigene Lage, Ausrichtung und Groesse (frueher hing er am Titel).
    pub(crate) hud_subtitle_x: f32,
    pub(crate) hud_subtitle_y: f32,
    pub(crate) hud_subtitle_align: u8,
    pub(crate) hud_subtitle_scale: u32,
    // Statuseffekte (Gui.extractEffects): normierte RECHTE Kante und Oberkante -- die Reihen
    // wachsen nach links wie in Vanilla. Groesse in Prozent der Hotbar-Skala.
    pub(crate) hud_effects_x: f32,
    pub(crate) hud_effects_y: f32,
    pub(crate) hud_effects_scale: u32,
    // Ob die Effekte als HUD-Anzeige erscheinen bzw. als Liste rechts neben dem Inventar
    // (EffectsInInventory).
    pub(crate) show_effects_hud: bool,
    pub(crate) show_effects_inventory: bool,
    // Stil der HUD-Anzeige: Vanilla (Rahmen mit Symbol) oder Detailliert (Symbol, Name, Timer
    // als Liste). Die detaillierte Liste hat ihre eigene Position: normiert die Kante, die
    // bei der gewaehlten Ausrichtung fest bleibt (s. hud_effects_rect), die andere Achse
    // links bzw. oben.
    pub(crate) effects_detailed: bool,
    // Ambiente Effekte (Leuchtfeuer, Conduit) nicht in der HUD-Anzeige.
    pub(crate) effects_hide_ambient: bool,
    pub(crate) hud_effects_detail_x: f32,
    pub(crate) hud_effects_detail_y: f32,
    // EFFECTS_ALIGNS: fester Rand, an dem die Liste beginnt.
    pub(crate) effects_align: u8,
    pub(crate) effects_icon: bool,
    pub(crate) effects_name: bool,
    // Name bzw. Timer in der Farbe des Effekts statt weiss.
    pub(crate) effects_name_color: bool,
    // EFFECTS_TIMER_STYLES.
    pub(crate) effects_timer_style: u8,
    pub(crate) effects_timer_color: bool,
    // In den letzten 10 Sekunden blinkt der Timer rot-weiss (s. EFFECTS_BLINK_TICKS):
    // EFFECTS_BLINK_MODES, Dynamisch nur bei Effekten, die laenger als 20 Sekunden kamen.
    pub(crate) effects_timer_blink: u8,
    // Dasselbe fuer die Actionbar (hud_actionbar_x ist damit der Anker, nicht mehr immer die
    // linke Kante). Standard links, damit bestehende Positionen gleich bleiben.
    pub(crate) hud_actionbar_align: u8,
    // Hotbar: hud_hotbar_x ist die normierte MITTE des Sheets, hud_hotbar_y seine normierte
    // UNTERKANTE (s. App::hud_hotbar_rect) -- damit bleibt der Standard 0.5/1.0 auf jeder
    // Aufloesung und bei jeder GUI-Skalierung mittig unten.
    pub(crate) hud_hotbar_x: f32,
    pub(crate) hud_hotbar_y: f32,
    // Bossbars: normierte MITTE und OBERKANTE (s. App::hud_bossbar_rect). Der Standard 0.5/0.0
    // ist genau Vanillas Lage.
    pub(crate) hud_bossbar_x: f32,
    pub(crate) hud_bossbar_y: f32,
    // Groesse in Prozent der GUI-Skala, wie bei den Effekten.
    pub(crate) hud_bossbar_scale: u32,
    // GUI-Skalierung der Hotbar: 0 = Auto (Vanillas "Auto", so gross wie das Grundraster
    // erlaubt, s. inventory::gui_scale), sonst fest (bis inventory::max_gui_scale).
    pub(crate) hud_hotbar_scale: u32,
    // Haltbarkeitsbalken in der Hotbar erst, wenn hoechstens so viel Prozent uebrig sind
    // (100 = immer, wie Vanilla).
    pub(crate) hotbar_durability_percent: u32,
    // Statt Prozent: Balken erst ab hoechstens so vielen verbleibenden Haltbarkeitspunkten
    // (Index in HOTBAR_DURABILITY_POINTS). Prozent warnt bei Gold zu spaet -- 10 % eines
    // goldenen Schwerts sind nur 3 Schlaege.
    pub(crate) hotbar_durability_absolute: bool,
    pub(crate) hotbar_durability_points: u32,
    // Luft als Balken im Stil des Saettigungsbalkens statt als Blasen (s. build_air_bar).
    pub(crate) hotbar_air_bar: bool,
    // Auswahlrahmen gleitet so viele ms zum neuen Slot, 0 = springt wie Vanilla (s. hotbar_glide).
    pub(crate) hotbar_glide_ms: u32,
    // Fluggeschwindigkeit als Zuschauer in Bloecken je Sekunde (eine der
    // spectator::SPEED_STEPS), ohne Anlauf und Auslauf.
    pub(crate) spectator_speed: u32,
    // Chat: normierte linke Kante und UNTERKANTE des Nachrichtenblocks (er waechst nach
    // oben).
    pub(crate) hud_chat_x: f32,
    pub(crate) hud_chat_y: f32,
    // Textgroesse des Chats in Prozent. 100 % = UI_SCALE_SMALL, die Groesse, die der Chat
    // immer hatte. Ersetzt das fruehere hud_chat_scale (Auto/x1..x4), s. Settings::parse.
    pub(crate) chat_scale_percent: u32,
    // Breite des geschlossenen Chats in Font-Pixeln (wird mit der Textgroesse mitskaliert).
    pub(crate) chat_width: u32,
    // Sichtbare Zeilen: geschlossen und waehrend man tippt (dann rueckt der Chat ueber die
    // Eingabezeile, s. chat_open_rect).
    pub(crate) chat_lines: u32,
    pub(crate) chat_lines_open: u32,
    // Deckkraft des Hintergrunds hinter den Chatzeilen in Prozent (0 = keiner).
    pub(crate) chat_background: u32,
    // Abdunkeln ueber der Eingabezeile bei offenem Chat, in Prozent (s. build_chat_overlay).
    pub(crate) chat_open_dim: u32,
    pub(crate) chat_fade_secs: u32,
    // Zusaetzlicher Abstand zwischen Chatzeilen, 0..100 % der Texthoehe (s. App::chat_line_height).
    pub(crate) chat_line_spacing: u32,
    // Chat im Spiel wie Bedrock: oben verankert statt unten (s. CHAT_TOOLTIPS).
    pub(crate) chat_bedrock: bool,
    // Index in CROSSHAIR_COLOR_MODES.
    pub(crate) crosshair_color_mode: usize,
    // Farbe im Modus Einfarbig: Ton in Grad, Saettigung und Helligkeit in Prozent.
    pub(crate) crosshair_hue: u32,
    pub(crate) crosshair_saturation: u32,
    pub(crate) crosshair_value: u32,
    // Aufhellung der Komplementaerfarbe in Prozent (0 = reine Gegenfarbe wie bisher, 100 =
    // Weiss). Nur im Komplementaer-Modus wirksam und deshalb auch nur dort bedienbar, s.
    // buttons()/build_hud_editor.
    pub(crate) crosshair_brightness_boost: u32,
    // Ein Pixel schwarze Kontur rings um das Fadenkreuz. Hilft vor allem dort, wo der
    // Untergrund zufaellig fast die Kreuzfarbe hat -- Schwarz daneben trennt es immer ab.
    // Gilt fuer JEDEN Farbmodus, nicht nur den Komplementaeren.
    pub(crate) crosshair_outline: bool,
    // Dieselbe Kontur um den Ring am Kreuz (Schild-Cooldown, Timer), eigens schaltbar.
    pub(crate) crosshair_ring_outline: bool,
    // Radius dieses Rings (Mitte der Linie) in Bildschirmpixeln.
    pub(crate) crosshair_ring_radius: u32,
    // Form des eingebauten Fadenkreuzes (Index in CROSSHAIR_SHAPES) und je Form die Groesse
    // in Pixeln -- jede Form behaelt ihre eigene, wenn man zwischen ihnen wechselt.
    pub(crate) crosshair_shape: usize,
    pub(crate) crosshair_sizes: [u32; CROSSHAIR_SHAPES.len()],
    // Eigenes Fadenkreuz aus dem Pixel-Editor statt des eingebauten (s. crosshair_editor).
    pub(crate) crosshair_custom: bool,
    pub(crate) crosshair_pattern: crosshair_editor::Pattern,
    // Abdunklung der Welt hinter dem offenen Inventar, in Prozent (s.
    // build_inventory_screen): 0 = keine Abdunklung, das Kamerabild bleibt voll sichtbar,
    // 100 = deckend schwarz. Das Fenster selbst ist davon nicht betroffen und bleibt immer
    // deckend.
    pub(crate) inventory_transparency: u32,
    // Legt man ein Item in einen Slot, springt der Zeiger in dessen Mitte (s. inventory_click).
    pub(crate) inventory_cursor_to_slot: bool,
    // GUI-Skalierung des Inventarfensters: 0 = Auto, sonst fest (bis inventory::max_gui_scale).
    pub(crate) inventory_scale: u32,
    // Vanillas Barrierefreiheits-Optionen "Glint Speed" und "Glint Strength" in Prozent
    // (GameOptions.glintSpeed/glintStrength, 0..1, Vorgaben 0.5 und 0.75): wie schnell der
    // Verzauberungs-Glint wandert und wie stark er leuchtet (s. render::glint).
    pub(crate) glint_speed: u32,
    pub(crate) glint_strength: u32,
    // Groesse der Spielerliste in Prozent der GUI-Skala (kein Vanilla), 100 = wie Vanilla.
    pub(crate) player_list_scale: u32,
    // Abdunklung des Weltbilds hinter den Menues, 0..100 %.
    pub(crate) menu_backdrop_opacity: u32,
    // Aussehen: wie traege Listen dem Mausrad nachgleiten, 0..100 % (s. scroll_inertia_rate).
    pub(crate) scroll_inertia: u32,
    // Aussehen: Scrollweite in Prozent (s. SCROLL_DISTANCE_DEFAULT).
    pub(crate) scroll_distance: u32,
    // Entities weiter weg als so viele Bloecke werden nicht gezeichnet (ENTITY_DISTANCE_MAX =
    // unbegrenzt), s. App::entity_in_render_distance.
    pub(crate) entity_render_distance: u32,
    // Block-Entities weiter weg als so viele Bloecke werden nicht gezeichnet
    // (BLOCK_ENTITY_DISTANCE_MAX = unbegrenzt), s. App::block_entity_shown.
    pub(crate) block_entity_render_distance: u32,
    // So lange blendet eine Spalte bei ihrem ersten Mesh ein, in ms (0 = aus), s.
    // render::BlockRenderer::start_column_fade.
    pub(crate) chunk_fade_ms: u32,
    // So lange vor ihrem Ende blenden Partikel aus, in ms (0 = aus), s. particles::Particles::fade_out_ms.
    pub(crate) particle_fade_out_ms: u32,
    // Bis so viele Bloecke um die Kamera entstehen Partikel (Vanilla 32), s.
    // particles::Particles::spawn_distance.
    pub(crate) particle_distance: u32,
    // Entities nah an der Kamera je Pixel rastern ab diesem Abstand, in Hundertstel Bloecken
    // (0 = aus), s. render::BlockRenderer::entity_near_dither.
    pub(crate) entity_near_dither_cm: u32,
    // Mip-Schaerfe der Welt-Texturen in Prozent (0 = Bias 0, 100 = Bias -1), s.
    // render::BlockRenderer::mip_bias.
    pub(crate) mip_sharpness_pct: u32,
    // Vertikales Sichtfeld in Grad (Vanilla-Default 70, Bereich FOV_MIN..FOV_MAX). Wird pro
    // Frame auf die Kamera uebertragen (s. render-Zweig), fliesst in die Perspektive UND als
    // tan(fov/2) in die nichtlinearen Projektionen.
    pub(crate) fov: f32,
    // Eigenes Sichtfeld je 3.-Person-Perspektive (s. Perspective). Getrennt von `fov`, damit
    // ein weiter Blick in der Verfolgeransicht die Ego-Perspektive nicht mitzieht.
    pub(crate) fov_third_back: f32,
    pub(crate) fov_third_front: f32,
    // 3.-Person-Ansichten, die die Perspektive-Taste ueberspringt (Knopf neben dem Sichtfeld).
    // Hoechstens eine: ist eine uebersprungen, bekommt die Taste H/T -- Halten schaut nur so
    // lange in die andere, wie sie gedrueckt ist (Quicklook, s. quicklook_target).
    pub(crate) skip_third_back: bool,
    pub(crate) skip_third_front: bool,
    // Kameraabstand je 3.-Person-Perspektive in Bloecken (Vanillas EntityAttributes::
    // CAMERA_DISTANCE, Default 4.0). An Waenden wird der Wert automatisch verkuerzt, s.
    // physics::clip_camera_distance -- er ist eine Obergrenze, kein fester Abstand.
    pub(crate) camera_distance_back: f32,
    pub(crate) camera_distance_front: f32,
    pub(crate) camera_y_offset_back: f32,
    pub(crate) camera_y_offset_front: f32,
    // Projektionsmodus, Index in PROJECTION_NAMES: 0=Normal, 1=Panini, 2=Stereografisch,
    // 3=Zylindrisch. Nicht-Normal warpt die Geometrie im Vertex-Shader (s. block.wgsl).
    pub(crate) projection_mode: u8,
    // Wohin der Riss gelegt wird, 0 = nicht steuern, 1 = Scanout-Timing.
    pub(crate) tear_position: u8,
    // Verschiebung des Bildwechsels in Mikrosekunden (negativ = frueher), s. App::tear_target_phase.
    pub(crate) tear_timing_us: i32,
    // Reserve, mit der ein Frame vor seinem Termin fertig sein soll (Mikrosekunden).
    pub(crate) tear_reserve_us: i32,
    // Partikel-Stufe, Index in PARTICLE_MODE_NAMES (0 = Alle, wie Vanilla).
    pub(crate) particles_mode: u8,
    // Partikel-Anpassung je Art (Index = particles::KINDS): an/aus, Dauer, Deckkraft, Tempo.
    pub(crate) particle_tuning: [particles::ParticleTuning; particles::KINDS.len()],
    // Dasselbe fuer die eigenen Partikel (an der eigenen Figur, s. particles::own_at); die
    // beiden oben gelten dann fuer die anderen. Ohne eigenen Eintrag in der Datei wie die oben.
    pub(crate) particles_mode_own: u8,
    pub(crate) particle_tuning_own: [particles::ParticleTuning; particles::KINDS.len()],
    // Hoehlen-Culling (s. render::occlusion). Nur zum Vergleichen abschaltbar.
    pub(crate) cave_culling: bool,
    // Entity-Culling (s. app/render/culling.rs). Nur zum Vergleichen abschaltbar.
    pub(crate) entity_culling: bool,
    // Stufe 2 davon: Sichtlinien (s. render::sight); wirkt nur mit entity_culling.
    pub(crate) entity_ray_culling: bool,
    // Mip-Stufen des Block-Atlas (aus = immer volle Aufloesung, s. render::BlockRenderer::mipmapping).
    pub(crate) mipmapping: bool,
    // Schrift, Index in FontKind::ALL (0 = Minecraft, 1 = DM Sans, 2 = Outfit, 3 = Lexend,
    // 4 = Lexend Exa, 5 = Noto Sans).
    pub(crate) font: u8,
    // Textschatten wie Vanilla (s. GpuState::shape_text).
    pub(crate) text_shadow: bool,
    // Art des Textrands, wenn text_shadow an ist: 0 = Kontur, 1 = Vanilla-Schatten.
    pub(crate) text_shadow_style: u8,
    // Wechselnde Zahlen halten ihren Platz (s. stable_numbers).
    pub(crate) stable_numbers: bool,
    // Schrift-Tab, Stil: Grossbuchstaben zeichnen (aus: aller Text klein, s. font_atlas::set_capitals).
    pub(crate) font_capitals: bool,
    // Je Schatten-Kaestchen (0 = Standard, 1 + Bereich): der Vanilla-Schatten einen halben
    // statt einen ganzen Schriftpixel versetzt.
    pub(crate) shadow_half: [bool; TEXT_AREA_COUNT + 1],
    // Leiste oben: unter der Uhr, wie lange der Client laeuft (Klick auf die Uhr schaltet um).
    pub(crate) topbar_uptime: bool,
    // Rand je Bereich, s. ShadowArea (0 = allgemein).
    pub(crate) area_shadow: [u8; TEXT_AREA_COUNT],
    // Schrift je Bereich, s. area_font_kind (0 = allgemein).
    pub(crate) area_font: [u8; TEXT_AREA_COUNT],
    // Staerke der Schatten-Kaestchen im Schrift-Tab: [0] der Standard, [1 + Bereich] die
    // Ausnahmen. Dicke der Kontur in Zehntelpixeln, Deckkraft der uebrigen Arten in Prozent
    // -- je nach Art gilt das eine oder das andere (s. Settings::shadow_strength).
    pub(crate) shadow_thickness: [u8; TEXT_AREA_COUNT + 1],
    pub(crate) shadow_opacity: [u8; TEXT_AREA_COUNT + 1],
    // Hintergrund des Startbildschirms, Index in wallpaper::WallpaperMode::ALL (0 = keins).
    pub(crate) wallpaper: u8,
    // Hintergrund Panorama: aus welchem Skin (Ordnername; leer oder ohne Panorama = der erste
    // mit Panorama) und wie schnell es sich dreht, in Prozent von Vanilla (negativ = andersherum).
    pub(crate) panorama_skin: String,
    pub(crate) panorama_speed: i32,
    // CMAA2 (Conservative Morphological Anti-Aliasing 2, s. cmaa2.wgsl) -- gather-basierte,
    // kontrastadaptive Kantenglaettung, inspiriert von Intels Referenzimplementierung (deren
    // Scatter/Deferred-Blend-Architektur noch nicht portiert ist, s. cmaa2.wgsl-Kommentar).
    pub(crate) cmaa2_enabled: bool,
    // Name des gewaehlten Ausgabegeraets (s. audio-Crate). Leer = System-Standard --
    // absichtlich kein Index, denn welche Geraete es gibt, weiss erst der naechste Start.
    pub(crate) audio_output_device: String,
    // Angefragter Ausgabepuffer in Frames, 0 = Vorgabe des Systems (s. AUDIO_BUFFER_CHOICES).
    // Privat wie das Geraet: was ohne Knacksen geht, haengt an Rechner und Kopfhoerer.
    pub(crate) audio_buffer_frames: u32,
    // Audio-Tab: PipeWire-Grenze und Prioritaet von pipewire-pulse selbst pflegen (s.
    // pipewire::Status::auto_optimieren).
    pub(crate) audio_auto_optimize: bool,
    // Master-Tonhoehe fuer UI-Klaenge aus Aufnahmen, in Halbtoenen (s. audio::semitone_factor).
    // Wirkt NUR auf den Aufnahme-Pfad von play_ui_sound, nicht auf Signaltoene wie den
    // Totem-Pop (audio::CueSink::play) -- der Regler ist zum Durchprobieren von
    // UI-Sounddesign gedacht, nicht zum Verstimmen von Gameplay-Signalen. Ganzzahlig wie im
    // Stepper von scripts/klaenge-anhoeren.sh, absichtlich ohne Ober-/Untergrenze.
    pub(crate) ui_pitch_semitones: i32,
    // Hauptlautstaerke in Prozent (s. App::play_server_sound), auch fuer die Klaenge der
    // Oberflaeche (Nutzer 2026-11-11; frueher bewusst ausgenommen).
    pub(crate) master_volume: f32,
    // Lautstaerke je Kategorie in Prozent, in der Reihenfolge von VOLUME_CATEGORIES.
    pub(crate) category_volume: [f32; VOLUME_CATEGORIES.len()],
    // Lautstaerke je UI-Klang in Prozent, in der Reihenfolge von ui_sound::UiSound::ALL --
    // zusaetzlich zur Kategorie UI-Sounds.
    pub(crate) ui_sound_volume: [f32; ui_sound::UiSound::ALL.len()],
    // --- Module (s. modules.rs) ---
    // Totem-Anzeige: unterscheidet beim Pop Haupthand und Zweithand nach Farbe, Seite und
    // Ton. Standardmaessig AN -- wer das Modul-Menue nicht findet, soll trotzdem die
    // brauchbarere Anzeige bekommen; sie ersetzt keine Vanilla-Funktion, die man vermissen
    // koennte (dieser Client hatte vorher gar keine Totem-Anzeige).
    pub(crate) module_totem_indicator: bool,
    pub(crate) module_totem_sound: bool,
    // Toteming Pro Max (s. toteming.rs): Totem-Warnung, Totem-Vorschau, Pop-Klaenge und
    // Pop-Optik. Eigene Unterstruktur, die sich selbst liest und schreibt.
    pub(crate) toteming: toteming::TotemingSettings,
    // Modul "Ghostblock-Indikator" (s. ModuleId::GhostTolerance, frueher "Vorhersage-
    // Toleranz"): eine eigene Vorhersage wird immer erst wie ein ECHTER Block gezeichnet.
    // AN heisst: hat der Server nach ghost_tolerance_ms immer noch nicht bestaetigt, wird sie
    // halbdurchsichtig -- ebenso ein abgebauter Block oder ein gezuendeter Anker, der noch
    // nicht weg ist. Dann ist die Vorhersage fraglich geworden, und genau DAS ist die
    // Auskunft, die man will. AUS = nie halbdurchsichtig, wie Vanilla.
    pub(crate) module_ghost_tolerance: bool,
    // Crystal-Optimizer (s. crystal_optimizer.rs): getroffene End-Kristalle sofort entfernen.
    pub(crate) module_crystal_optimizer: bool,
    // Consumable Optimizer (s. consumable_optimizer.rs): Essen endet im Client ohne Warten.
    pub(crate) module_consumable_optimizer: bool,
    // Consumable-Optimizer: laesst man beim Essen/Trinken los und fehlen hoechstens so viele
    // Millisekunden, haelt der Client bis zum Ende durch (0 = aus).
    pub(crate) eat_hold_through_ms: u32,
    // Consumable-Optimizer: Slotwechsel, Tausch und Wegwerfen warten, solange der Server noch
    // isst (s. App::server_still_eating).
    pub(crate) eat_slot_lock: bool,
    // Usage-Optimizer "No Shield Lock": haelt das Schild der Nebenhand den Rechtsklick fest,
    // obwohl die Haupthand inzwischen etwas Benutzbares haelt, endet das Blocken von selbst
    // (s. App::shield_locks_main_hand).
    pub(crate) no_shield_lock: bool,
    // Timer am Fadenkreuz: Kreis fuellt sich beim Spannen/Essen (s. App::use_timer_ring).
    pub(crate) module_use_timer: bool,
    // No Tool Break (s. App::tool_break_guard): Abbauen sperren, bevor ein Diamant- oder
    // Netherit-Werkzeug mit Reparatur zerbricht, und ab welcher Resthaltbarkeit.
    pub(crate) module_no_tool_break: bool,
    pub(crate) no_tool_break_reserve: u32,
    // Dynamisches Feuer (Nachbau der dynamicfire-Mod des Nutzers): mit Feuerresistenz blenden
    // Feuerbloecke aus, s. App::fire_alpha.
    pub(crate) module_dynamic_fire: bool,
    // Zoom-Modul: Staerke in Zehnteln (40 = Sichtfeld durch 4) und der Faktor auf die
    // Maus-Empfindlichkeit beim vollen Zoom in Prozent.
    pub(crate) module_zoom: bool,
    // Sprachchat (Simple-Voice-Chat-kompatibel, s. voicechat.rs).
    pub(crate) module_voice_chat: bool,
    pub(crate) voice: voicechat::VoiceSettings,
    pub(crate) zoom_factor_tenths: u32,
    pub(crate) zoom_sens_percent: u32,
    // Eingaben des Sens-Rechners, gemerkt fuers naechste Oeffnen.
    pub(crate) sens_calc_mc_percent: f32,
    pub(crate) sens_calc_dpi: u32,
    // Botcheck-Pass (s. App::vanilla_level_ready): beim Betreten einer Welt Vanillas
    // Zeitachse einhalten -- erst laden, dann loslaufen.
    pub(crate) module_botcheck_pass: bool,
    // Wie lange eine unbestaetigte Platzierung als echt gilt. Standard 200 ms -- etwa eine
    // normale Rundreise; wer auf einem weiter entfernten Server spielt, dreht hoch.
    pub(crate) ghost_tolerance_ms: u32,
    // Hitbox-Anzeige: zeichnet die Trefferbox anderer Entities als Drahtgitter an ihrer
    // GEGLAETTETEN Bildposition (render_pos, s. world::Entity). Nur beim Spieler waehlbar:
    // stattdessen an der ROHEN, zuletzt vom Server geschickten Position (target_pos) -- die
    // zeigt, wie weit die Glaettung dem echten Server-Stand gerade hinterherhaengt.
    // Standardmaessig AUS: ein neues Debug-/Analyse-Modul, keine bestehende Funktion, die man
    // vermissen wuerde.
    pub(crate) module_hitbox_enabled: bool,
    pub(crate) module_hitbox_player_raw: bool,
    // Gestrichelt statt durchgezogen (s. world::wireframe_box_vertices).
    pub(crate) module_hitbox_dotted: bool,
    // Welche Kanten (HITBOX_LINE_NAMES): alle, nur die, die die Box selbst nicht verdeckt, oder
    // nur der Umriss (s. build_hitbox_vertices).
    // Die eigene Hitbox (nur in der 3. Person, s. build_hitbox_vertices).
    pub(crate) module_hitbox_self: bool,
    // Kantenbreite in Bildschirmpixeln -- IMMER wirksam (auch durchgezogen), im gepunkteten
    // Modus zusaetzlich Strich+Luecke-Laenge. s. App::build_hitbox_vertices fuer die
    // Umrechnung in Weltlaenge bei der aktuellen Entfernung des jeweiligen Entities.
    pub(crate) module_hitbox_density_px: u32,
    // Deckkraft in Prozent, wie inventory_transparency.
    pub(crate) module_hitbox_opacity: u32,
    // Farbe je Variante als HSV (s. hsv_to_rgb) -- getrennt fuer geglaettet und roh, damit
    // beide gleichzeitig sichtbaren Boxen sich unterscheiden lassen. Ton in Grad (0..360),
    // Saettigung/Helligkeit in Prozent.
    pub(crate) module_hitbox_smoothed_hue: u32,
    pub(crate) module_hitbox_smoothed_saturation: u32,
    pub(crate) module_hitbox_smoothed_value: u32,
    pub(crate) module_hitbox_raw_hue: u32,
    pub(crate) module_hitbox_raw_saturation: u32,
    pub(crate) module_hitbox_raw_value: u32,
    // Eigene Arten (s. hitbox_kinds): je Art an/aus, Farbe und Deckkraft.
    pub(crate) module_hitbox_styles: [hitbox_kinds::HitboxStyle; hitbox_kinds::HitboxKind::COUNT],
    // Teams-Modul (s. teams.rs): Hitboxen von Verbuendeten und Gegnern, je Seite (Side::index).
    pub(crate) module_teams: bool,
    pub(crate) team_hitboxes: [teams::TeamHitbox; 2],
    // Woran Verbuendete erkannt werden (s. teams::Matchers).
    pub(crate) team_match: teams::Matchers,
    // Eigene Teamfarbe (0xRRGGBB), None = automatisch erkennen. Nicht jeder Server zeigt den
    // eigenen Namen richtig gefaerbt an (Seitenleiste in der falschen Farbe).
    pub(crate) team_own_color: Option<u32>,
    // Schadens-Faerbung ("damage tint"): die Farbe, in der ein Lebewesen aufblitzt, wenn es
    // Schaden nimmt. Ton in Grad, Saettigung/Helligkeit/Deckkraft in Prozent.
    //
    // Vorgabe ist EXAKT Vanilla: reines Rot (Ton 0, volle Saettigung und Helligkeit) mit 30 %
    // Deckkraft. Vanillas OverlayTexture traegt in der Hurt-Zeile ARGB(178, 255, 0, 0), und
    // entity.fsh mischt `mix(overlayColor.rgb, color.rgb, overlayColor.a)` -- der Rot-Anteil
    // ist also 1 - 178/255 = 30.2 %. Auf 30 gerundet, weil die Einstellung in ganzen Prozent
    // laeuft und der Unterschied unter einem Farbwert von 255 liegt.
    // Crit-Hitcolor-Modul (s. CRIT_COLOR_ROW): eine ZWEITE Schadens-Faerbung, die nur
    // greift, wenn der Treffer ein kritischer war. Dieselben vier Achsen wie die gewoehnliche,
    // damit sie sich auch in der Deckkraft unterscheiden koennen -- ein Crit soll sich
    // abheben duerfen, nicht nur anders getoent sein.
    pub(crate) module_crit_color_enabled: bool,
    pub(crate) module_crit_color_hue: u32,
    pub(crate) module_crit_color_saturation: u32,
    pub(crate) module_crit_color_value: u32,
    pub(crate) module_crit_color_alpha: u32,
    // Blockumriss-Modul (s. ModuleId::BlockOutline): AUS gar kein Rahmen um den anvisierten
    // Block (Nutzer 2026-11-11), AN mit eigener Farbe, Deckkraft und Linienbreite.
    pub(crate) module_block_outline: bool,
    // Modul "Saettigung": ein Balken in eigener Farbe ueber der Hungerleiste (s.
    // build_status_bars).
    pub(crate) module_saturation_hud: bool,
    // Farbe der Stueckzahl im Slot (Ton, Saettigung, Helligkeit): 0 = Hotbar, 1 = Inventar und
    // alle anderen Slots.
    pub(crate) stack_count_colors: [[u32; 3]; 2],
    pub(crate) saturation_hud_hue: u32,
    pub(crate) saturation_hud_saturation: u32,
    pub(crate) saturation_hud_value: u32,
    pub(crate) saturation_hud_height: u32,
    // Deckkraft in Prozent: der schwarze Grund innerhalb des Rahmens und das Raster (Rahmen
    // und Trennstriche).
    pub(crate) saturation_hud_bg_alpha: u32,
    pub(crate) saturation_hud_grid_alpha: u32,
    // Modul "Eigenname" (s. ego_name): Alias (leer = Name bleibt), Verzierung, Farben als
    // (Ton, Saettigung, Helligkeit, Deckkraft) -- erste, zweite Farbe, Schatten --, eigener
    // Schatten an/aus, Tempo in Zehntel-Umlaeufen je Sekunde, Ton bei Erwaehnung ("" = aus)
    // und dessen Lautstaerke in Prozent.
    pub(crate) module_ego_name: bool,
    pub(crate) ego_alias: String,
    pub(crate) ego_deco: ego_name::Deco,
    // Farben (Ton, Saettigung, Helligkeit, Deckkraft): Name 1 und 2, Schatten 1 und 2. Die
    // Deckkraft gilt nur beim ersten Schatten, fuer den ganzen Schatten.
    pub(crate) ego_colors: [[u32; 4]; 4],
    pub(crate) ego_shadow: ego_name::Deco,
    // Aufhellung des Regenbogens zu Weiss hin in Prozent, beim Namen und beim Schatten.
    pub(crate) ego_light: u32,
    pub(crate) ego_shadow_light: u32,
    pub(crate) ego_speed: u32,
    pub(crate) ego_sound: String,
    pub(crate) ego_volume: f32,
    // Einstellungen der Addon-Module (s. addons.rs) als Text, Schluessel "addon/modul/key";
    // "addon/modul/enabled" ist ihr Hauptschalter. In der Datei als addon:<Schluessel>=Wert.
    pub(crate) addon_values: std::collections::BTreeMap<String, String>,
    pub(crate) block_outline_hue: u32,
    pub(crate) block_outline_saturation: u32,
    pub(crate) block_outline_value: u32,
    pub(crate) block_outline_alpha: u32,
    pub(crate) block_outline_width_px: u32,
    // Kein Rahmen um den Block, den man gerade abbaut (nur mit eingeschaltetem Modul) -- die
    // Risse sind dann ohnehin die deutlichere Markierung.
    pub(crate) block_outline_hide_mining: bool,
    // Rahmen in der Gegenfarbe des Bildes darunter statt in der eigenen Farbe (wie das
    // Fadenkreuz im Modus Komplementaer); die Deckkraft bleibt die eingestellte.
    pub(crate) block_outline_complementary: bool,
    // Himmelsfarben-Modul (s. ModuleId::SkyColor): AUS folgt der Himmel der Tageszeit
    // (world::sky), AN steht er fest auf dieser Farbe -- Tag wie Nacht.
    pub(crate) module_sky_color_enabled: bool,
    // Im Modul: die Farbe festhalten (bisheriges Verhalten) und/oder die Uhrzeit des Himmels.
    // Die feste Uhrzeit gilt nur fuers Bild -- Himmelsfarbe und Pack-Himmel (s. custom_sky)
    // --, nicht fuer die Welt: der Server bestimmt weiter Licht und Mobs.
    pub(crate) sky_mode: SkyMode,
    // Verlauf: Sonne und Mond zeigen.
    pub(crate) sky_sun: bool,
    pub(crate) sky_moon: bool,
    pub(crate) sky_stars: bool,
    pub(crate) module_sky_fixed_time: bool,
    // Uhrzeit in Ticks des Tages (0 = 6:00, 6000 = Mittag).
    pub(crate) module_sky_time: u32,
    // Modul "Wetter" (s. WeatherMode): AUS zeigt es das Wetter des Servers mit Vanillas
    // Vorgabe-Radius. Der Radius ist Vanillas Option "Wetterradius" (Bloecke um die Kamera).
    pub(crate) module_weather: bool,
    pub(crate) weather_mode: WeatherMode,
    // Radius der Kugel um die Kamera (alle drei Achsen), in der Regen faellt.
    pub(crate) weather_radius: u32,
    // Schnee: Bloecke mit hoechstens so viel freier Strecke zum Auge, und nie weiter als der
    // maximale Schneeradius (nie kleiner als der Schneeradius, s. couple_snow_radius).
    pub(crate) weather_snow_radius: u32,
    pub(crate) weather_snow_radius_max: u32,
    // Menge der Regentropfen und Flocken in Prozent (s. precipitation::DENSITY_DEFAULT).
    pub(crate) weather_density: u32,
    // Fallgeschwindigkeit des Schnees in Zehntel Bloecken je Sekunde (s. precipitation).
    pub(crate) weather_snow_speed: u32,
    pub(crate) module_sky_color_hue: u32,
    pub(crate) module_sky_color_saturation: u32,
    pub(crate) module_sky_color_value: u32,
    pub(crate) damage_tint_hue: u32,
    pub(crate) damage_tint_saturation: u32,
    pub(crate) damage_tint_value: u32,
    pub(crate) damage_tint_alpha: u32,
    // Faerbt der Schadensblitz nur den Skin/Koerper (Vanilla, Vorgabe) oder auch Ruestung,
    // Items, Schild und Umhang?
    pub(crate) damage_tint_skin_only: bool,
    // Dauer des Schadensblitzes in Ticks (Vanilla: hurtTime = 10).
    pub(crate) damage_tint_ticks: u32,
    // Pfeil-Modul (s. ModuleId::Arrows): Munitionsanzeige und Flugbahnen.
    pub(crate) module_arrows: bool,
    // Ruestungs-HUD (s. ModuleId::ArmorHud, armor_hud.rs): obere linke Ecke normiert, Groesse in
    // Prozent, Ausrichtung (armor_hud::LAYOUT_*) und wohin die Teile ruecken, wenn eins fehlt
    // (0 = oben/links, 1 = unten/rechts) -- je eigener Wert fuer hochkant und flach.
    pub(crate) module_armor_hud: bool,
    pub(crate) hud_armor_x: f32,
    pub(crate) hud_armor_y: f32,
    pub(crate) hud_armor_scale: u32,
    pub(crate) armor_hud_layout: u8,
    // Abstand zwischen den Teilen in Bildpixeln, Groesse der Zahl in Prozent und wo sie steht
    // als Verschiebung gegen die Bildmitte in Bildpixeln (x, y; s. armor_hud::number_origin).
    pub(crate) armor_hud_gap: i32,
    pub(crate) armor_hud_number_scale: u32,
    pub(crate) armor_hud_number_x: i32,
    pub(crate) armor_hud_number_y: i32,
    pub(crate) armor_hud_sort_vertical: u8,
    pub(crate) armor_hud_sort_flat: u8,
    // Haltbarkeitszahl gruen, solange ueber 87 % uebrig sind.
    pub(crate) armor_hud_green: bool,
    // Ring ums Fadenkreuz: wie viel Haltbarkeit die getragenen Teile mit Mending zusammen
    // noch haben (Summe der Reste durch Summe der Hoechstwerte).
    pub(crate) armor_hud_ring: bool,
    // Keystrokes (s. ModuleId::Keystrokes, keystrokes.rs): obere linke Ecke normiert, Groesse
    // in Prozent und Deckkraft der nicht gedrueckten Tasten.
    pub(crate) module_keystrokes: bool,
    // Hurt-Cam (Video-Tab): Rollen bei eigenem Schaden, abwechselnd links und rechts, und wie
    // stark in Prozent von Vanillas Ausschlag (HURT_CAM_MAX_DEGREES).
    pub(crate) hurt_cam: bool,
    pub(crate) hurt_cam_strength: u32,
    // Schulterkamera (Video-Tab, wie die Mod Shoulder Surfing Reloaded): die 3. Person von
    // hinten sitzt seitlich ueber der Schulter, `shoulder_offset_cm` in Hundertstel Bloecken
    // (positiv = rechts). Dazu ein Fadenkreuz dort, wo der Spieler wirklich hinzielt, der
    // eigene Spieler blendet aus, wenn die Kamera nah an ihm ist, und auf Wunsch dreht die
    // Kamera frei vom Koerper (der Spieler laeuft in Bewegungsrichtung, s. App::cam_look).
    pub(crate) shoulder_cam: bool,
    pub(crate) shoulder_offset_cm: i32,
    pub(crate) shoulder_crosshair: bool,
    pub(crate) shoulder_fade: bool,
    pub(crate) shoulder_decoupled: bool,
    // Freelook-Modul: solange die Taste aktiv ist, dreht die Maus nur die Kamera. In welcher
    // Ansicht (s. FREELOOK_VIEW_NAMES).
    pub(crate) module_freelook: bool,
    pub(crate) freelook_view: u8,
    // Trims auf getragener Ruestung zeichnen (Video, Darstellung).
    pub(crate) armor_trims: bool,
    // Render-Order: was immer ueber was liegt (s. render_order).
    pub(crate) render_order: crate::render_order::RenderOrderTable,
    // Modified View Model (Video, Darstellung): Arm haelt das Item, Hand-Debug-Werte je Item.
    pub(crate) modified_view_model: bool,
    // Hand-Bobbing (Video, Modified View Model): Vanillas Wackeln beim Laufen nur an der Hand,
    // die Kamera bleibt ruhig.
    pub(crate) view_model_bobbing: bool,
    // Feuer vor den Augen (Video-Tab, FIRE_MODE_*): Vanillas Bild mit eigener Hoehe in Prozent,
    // oder statt des Bilds ein Fadenkreuz in eigener Farbe (HSV, wie crosshair_*).
    pub(crate) fire_mode: u8,
    pub(crate) fire_height: u32,
    pub(crate) fire_crosshair_hue: u32,
    pub(crate) fire_crosshair_saturation: u32,
    pub(crate) fire_crosshair_value: u32,
    pub(crate) hud_keys_x: f32,
    pub(crate) hud_keys_y: f32,
    pub(crate) hud_keys_scale: u32,
    pub(crate) keys_opacity: u32,
    // Session-Info (s. ModuleId::SessionInfo, session_info.rs): obere linke Ecke normiert,
    // Groesse und Deckkraft wie bei den Keystrokes, dazu welche Zeilen unter der Spielzeit
    // stehen (session_info::LINE_*).
    pub(crate) module_session_info: bool,
    // Video > Tod: wie ein Lebewesen stirbt (DEATH_ANIM_*).
    pub(crate) death_animation: u8,
    pub(crate) hud_session_x: f32,
    pub(crate) hud_session_y: f32,
    pub(crate) hud_session_scale: u32,
    pub(crate) session_opacity: u32,
    pub(crate) session_lines: u8,
    // Uhr (s. ModuleId::Clock): echte Uhrzeit, obere linke Ecke normiert und Groesse in Prozent.
    pub(crate) module_clock: bool,
    pub(crate) hud_clock_x: f32,
    pub(crate) hud_clock_y: f32,
    pub(crate) hud_clock_scale: u32,
    // Rueckmeldung, wenn man ohne Munition den Bogen/die Armbrust zu spannen versucht (Vanilla:
    // es passiert schlicht nichts). Index in ARROW_EMPTY_FEEDBACK_NAMES.
    pub(crate) arrow_empty_feedback: u8,
    // Pfeil-Modul: beim Spannversuch ohne Munition den UI-Klang "Loeschen" spielen.
    pub(crate) arrow_empty_sound: bool,
    // Debug-Modul "Barrieren anzeigen": Barriere, Lichtblock und Strukturleere als Wuerfel mit
    // ihrer Partikeltextur zeichnen, statt sie wie Vanilla unsichtbar zu lassen.
    pub(crate) module_show_markers: bool,
    // Debug-Modul "Sound-Debug": zwei Balken mit der Lautstaerke des linken und rechten Kanals.
    pub(crate) module_sound_debug: bool,
    // Hand-Debug: die ganze Ego-Hand nach links oben (s. build_hand_vertices).
    pub(crate) module_hand_debug: bool,
    // Je Item (Registry-Name): Verschiebung der Ego-Faust gegen ihren berechneten Platz in
    // Tausendstel Bloecken im Sichtraum (x rechts, y oben, z zur Kamera), dann die Drehung des
    // Arms um die Faust in Grad (kippen, schwenken, rollen) und um seine eigene Laenge, zuletzt
    // die Drehung der Schulter um die Faust in Grad (x, y, z), dann die Verschiebung des Items
    // selbst in Tausendstel Bloecken (der Arm folgt seinem Griff). Bei
    // Hand-Debug mit den Pfeiltasten einstellbar (s. App::hand_debug_key), damit es ohne neuen
    // Build ausprobiert werden kann.
    pub(crate) hand_fist_items: std::collections::BTreeMap<String, [i32; 16]>,
    // Debug-Modul "Connection Debug": Lag-backs und starkes Rubberbanding als Benachrichtigung.
    // "Keine Pakete vom Server" kommt immer.
    pub(crate) module_connection_debug: bool,
    // Delay (s. net::lag): lag_delay_ms je Richtung auf der Leitung, nur lokal und auf anticheat-test.com.
    pub(crate) module_delay: bool,
    pub(crate) lag_delay_ms: u32,
    // Discord-Status (s. discord_rpc): an/aus und die Application-ID der eigenen
    // Discord-Anwendung -- deren NAME steht hinter "Spielt".
    pub(crate) module_discord_status: bool,
    // Modul "input-log" (s. input_log.rs): schreibt die per Raw-Input erkannten Spiel-
    // Eingaben mit Zeitstempel in eine lokale Datei. Vorgabe aus -- ein Input-Protokoll laeuft
    // nur auf ausdruecklichen Wunsch.
    pub(crate) module_input_log_enabled: bool,
    // Modul "coral-record" (s. crates/record, app/record.rs): Aufnahme fuer spaetere Videos.
    // Vorgabe aus.
    pub(crate) module_coral_record: bool,
    // Modul "coral-clip" (s. app/clip.rs): haelt die letzten clip_seconds Sekunden im Speicher,
    // die Taste "Clip speichern" schreibt sie als Aufnahme. Vorgabe aus.
    pub(crate) module_coral_clip: bool,
    pub(crate) clip_seconds: u32,
    // Regelmaessige Haltepunkte je Clip-Dauer (4 = alle Viertel der Dauer). An einem Haltepunkt
    // kann ein Clip beginnen; er wird hoechstens um einen Abstand laenger als eingestellt.
    pub(crate) clip_checkpoints: u32,
    // Debug-Modul "Autoklicker" (s. App::tick_autoclicker): solange Angreifen oder Benutzen gehalten ist,
    // klickt es autoclicker_cps-mal je Sekunde. Wirkt nur auf anticheat-test.com und lokalen
    // Servern (s. App::autoclicker_allowed) -- zum Testen der Anticheats, nicht zum Spielen.
    pub(crate) module_autoclicker: bool,
    pub(crate) autoclicker_cps: u32,
    // Modul "Farbe" (s. color.wgsl): Saettigung des ganzen Bildes in Prozent, 0 = schwarzweiss,
    // 100 = unveraendert.
    pub(crate) module_color: bool,
    // Modul Nachhaltigkeit: im Hintergrund keine Bilder (sustain_background), hinter dem
    // Pausemenue eine stehende Welt (sustain_pause). Beides nur bei eingeschaltetem Modul.
    pub(crate) module_sustainability: bool,
    pub(crate) sustain_background: bool,
    pub(crate) sustain_pause: bool,
    // Mehr Animationen (Video-Tab, s. world::more_anims): welche Animationen an sind (Bits),
    // und der Hauptschalter davor -- aus ist keine an, die Auswahl bleibt erhalten.
    pub(crate) anim_flags: u32,
    pub(crate) anims_enabled: bool,
    // Modul Chunk-Saver (s. lod.rs): Chunks speichern, mit so vielen Threads, und das
    // Gespeicherte auch in der normalen Sichtweite zeichnen, bis die echten Chunks da sind
    // (schon beim Verbinden, s. lod::App::lod_preview_saved).
    pub(crate) module_chunk_saver: bool,
    pub(crate) chunk_saver_threads: u32,
    pub(crate) chunk_saver_render_saved: bool,
    // Fernsicht aus dem Gespeicherten (Video-Tab): Weite in Chunks (0 = aus), Unterteilungs-
    // schwelle in Pixel^2.
    pub(crate) lod_render_distance: u32,
    pub(crate) chunk_saver_threshold: u32,
    // Vanillas Option "Operator-Items-Reiter": im Kreativmodus mit Operator-Rechten den Reiter
    // mit Befehlsbloecken & Co. zeigen.
    pub(crate) creative_op_tab: bool,
    // Sonstiges > Server-Bildschirme: welche offenen Bildschirme ClientboundContainerClose
    // NICHT schliesst (Bit = Zeile, s. SERVER_CLOSE_*_ROW und ClientEvent::CloseScreen) und
    // was mit vom Server geoeffneten passiert (SERVER_OPEN_* je Eintrag, s. SERVER_OPEN_SIGN).
    pub(crate) server_close_blocked: u8,
    pub(crate) server_open: [u8; SERVER_OPEN_COUNT],
    // Sonstiges: der Server kann den Chat nicht leeren -- weder mit Reset Chat noch mit den
    // Leerzeilen, die Clear-Chat-Plugins schicken (s. push_chat_message).
    pub(crate) no_chat_clear: bool,
    // Sonstiges: Schilder direkt in der Welt beschreiben (s. sign_edit_in_world). Aus = immer
    // das Editor-Fenster wie Vanilla.
    pub(crate) sign_edit_in_world: bool,
    // Sonstiges: was mit dem Hotbar-Slot bzw. der Blickrichtung passiert, die der Server
    // setzt (s. SERVER_SET_*): uebernehmen wie Vanilla, ignorieren, oder ignorieren und den
    // eigenen Wert zurueckschicken.
    pub(crate) no_slot_set: u8,
    pub(crate) no_rotate_set: u8,
    // Inventar > Animation: die Hervorhebung des Slots unter dem Zeiger blendet in so vielen ms
    // ein und klingt danach aus wie die Menueknoepfe (0 = sofort, wie Vanilla), in dieser Farbe (Farbton, Saettigung, Helligkeit,
    // Deckkraft; Vanilla: weiss, s. slot_hover_rgba).
    pub(crate) slot_hover_fade_ms: u32,
    pub(crate) slot_hover_color: [u32; 4],
    // Inventar > Animation: verschobene Items fliegen in so vielen ms zu ihrem neuen Platz
    // (0 = aus, sie springen wie in Vanilla; s. inventory_flight).
    pub(crate) swap_anim_ms: u32,
    // Totems fliegen dabei nicht, sie springen wie in Vanilla (s. Flights::step).
    pub(crate) swap_anim_skip_totems: bool,
    // Dasselbe fuer Heiltraenke (world::potion::is_healing_potion).
    pub(crate) swap_anim_skip_healing: bool,
    // Feld hinter der Spielerfigur im Inventar: true frei (die Welt scheint durch), false
    // Vanillas schwarzes Feld (s. build_inventory_screen).
    pub(crate) doll_backdrop_clear: bool,
    // Buchstaben auf Traenken im Inventar (s. world::potion::variant_mark), je einzeln:
    // S = verstaerkt, L = verlaengert, W = schwache Heilung/Schaden.
    pub(crate) potion_mark_strong: bool,
    pub(crate) potion_mark_long: bool,
    pub(crate) potion_mark_weak: bool,
    pub(crate) color_saturation: u32,
    pub(crate) discord_client_id: String,
    pub(crate) arrow_indicator: bool,
    // Nur die Zahl zeigen, ohne das Pfeil-Icon.
    pub(crate) arrow_number_only: bool,
    // Faerbung der Pfeil-Zahl, Index in HUD_TEXT_STYLE_NAMES (Schrift: area_font).
    pub(crate) arrow_text_style: u8,
    // Deckkraft der Anzeige in Prozent.
    pub(crate) arrow_opacity: u32,
    // Mitte der Anzeige, normiert (HUD-Editor).
    pub(crate) hud_arrows_x: f32,
    pub(crate) hud_arrows_y: f32,
    pub(crate) arrow_trail: bool,
    // Pfeile in der Welt ganz weiss zeichnen (Pfeile-Modul), damit man sie sofort sieht.
    pub(crate) arrow_white: bool,
    // Krit-Spur fliegender Pfeile (Pfeile-Modul); aus = keine Krit-Partikel hinter Pfeilen.
    pub(crate) arrow_crit_trail: bool,
    // Standzeit eines Spurpunkts bis zum Ausblenden und Dauer des Ausblendens (Zehntelsekunden).
    pub(crate) trail_hold_tenths: u32,
    pub(crate) trail_fade_tenths: u32,
    // Schild-Modul (s. ModuleId::Shields): Hauptschalter und die drei Teile.
    pub(crate) module_shields: bool,
    pub(crate) shield_cd_self: bool,
    pub(crate) shield_cd_players: bool,
    pub(crate) shield_hold_players: bool,
    // Hitbox-Farbe fuer Spieler mit Schild-Cooldown (HSV).
    pub(crate) shield_cd_hue: u32,
    pub(crate) shield_cd_saturation: u32,
    pub(crate) shield_cd_value: u32,
    // Farbfilter fuer Spieler mit gehobenem Schild (HSV + Deckkraft, wie die Schadens-Faerbung).
    pub(crate) shield_hold_hue: u32,
    pub(crate) shield_hold_saturation: u32,
    pub(crate) shield_hold_value: u32,
    pub(crate) shield_hold_alpha: u32,
    // Deckkraft der Cooldown-Farbe, wenn sie als Modell-Filter wirkt (s. ShieldMark).
    pub(crate) shield_cd_alpha: u32,
    // Wie Cooldown/Hochhalten an anderen Spielern gezeigt werden, ShieldMark::index.
    pub(crate) shield_cd_mark: u8,
    pub(crate) shield_hold_mark: u8,
    // Eigener Cooldown: Index in SHIELD_SELF_MODE_NAMES und Faerbung der Zahl (Index in
    // HUD_TEXT_STYLE_NAMES). Die Schrift steht in area_font.
    pub(crate) shield_cd_self_mode: u8,
    pub(crate) shield_cd_text_style: u8,
    // Mitte der eigenen Schild-Cooldown-Zahl, normiert (HUD-Editor).
    pub(crate) hud_shield_cd_x: f32,
    pub(crate) hud_shield_cd_y: f32,
    // Vanillas pseudozufaellige Textur-Rotation/-Spiegelung fuer bestimmte Vollwuerfel
    // (Erde, Sand, Stein, Grasblock, ...), s. world::block_uvs::BlockUvs::set_rng_rotation_enabled
    // und den Kommentar dort zur genauen Herleitung aus dem Yarn-Quellcode. Standard AN (wie
    // Vanilla), abschaltbar auf Nutzerwunsch.
    pub(crate) block_rng_rotation: bool,
    // Vanillas "Weiche Beleuchtung": die Eckabdunklung an Kanten und in Nischen (s.
    // world::ambient_occlusion). Bewusst NUR dieser Teil -- Lichtwerte gibt es in diesem
    // Client nicht, er rendert absichtlich fullbright. In Prozent der Vanilla-Abdunklung:
    // 0 = aus, 100 = wie Vanilla (Standard).
    pub(crate) smooth_lighting_percent: u32,
    // Bewegungsunschaerfe der Kamera (render::motion_blur) in Prozent: 100 = die Bewegung
    // waehrend eines Monitorbilds, 0 = aus.
    pub(crate) motion_blur_percent: u32,
    // Dasselbe fuer die eigene Bewegung der Entities (render::motion_blur, entity_motion),
    // unabhaengig davon; 0 = sie verwischen nur mit der Kamera wie die uebrige Welt.
    pub(crate) motion_blur_entities_percent: u32,
    // Unschaerfe der Hand samt Items (render::hand_blur) in Prozent wie motion_blur_percent,
    // 0 = aus; Gewichtung der Teilbilder als Index in HAND_BLUR_CURVES.
    pub(crate) hand_blur_percent: u32,
    pub(crate) hand_blur_curve: u8,
    // physics::PlayerPhysics::jump_cooldown (Vanillas LivingEntity.jumpingCooldown, s.
    // dortigen Kommentar): 10-Tick-Abklingzeit zwischen zwei echten Spruengen, geteilt
    // zwischen Land- und Wassersprung. An Land praktisch unsichtbar, im flachen Wasser
    // deutlich spuerbar. NICHT zu verwechseln mit "water hopping": das ist
    // resetVerticalVelocityInFluid (s. physics::jump_out_of_fluid), hat gar keine
    // Abklingzeit und haengt nicht an dieser Einstellung. Manche
    // Spieler empfinden diese Verzoegerung als stoerend (z.B. schnelles Wasser-Ueberqueren) --
    // togglebar auf Nutzerwunsch, Standard AN (wie Vanilla).
    pub(crate) jump_cooldown_enabled: bool,
    // Sprint-Umschalten startet eingeschaltet (nur mit Sprinten auf "Umschalten").
    pub(crate) sprint_on_start: bool,
    // Feststelltaste schaltet im Spiel keine Grossschreibung um (s. caps_lock.rs).
    pub(crate) caps_lock_blocked: bool,
    // Sneak-Kantenschutz (Vanillas adjustMovementForSneaking / clipAtLedge()==isSneaking()):
    // verhindert das Herunterfallen von einer Blockkante beim Schleichen. Togglebar auf
    // Nutzerwunsch, Standard AN (wie Vanilla). Grim flaggt weder AN noch AUS.
    pub(crate) sneak_edge_protection_enabled: bool,
    // SnapTap/SOCD: bei zwei gegensaetzlichen Bewegungstasten gewinnt die zuletzt gedrueckte,
    // statt dass sie sich gegenseitig aufheben (s. SNAP_TAP_TOOLTIP). Standard aus -- das
    // ist bewusst KEIN Vanilla-Verhalten, und etliche Server verbieten es per Regelwerk.
    pub(crate) snap_tap_enabled: bool,
    // Wiederholt eine gehaltene Benutzen-Taste die Benutzung (Vanilla: ja, alle 4 Ticks)?
    // NIE macht aus dem Halten einen einzelnen Klick, AUSSER BLOECKE nur fuer Haende mit einem
    // Block -- gedacht fuer das Setzen EINES Blocks, ohne dass beim Loslassen versehentlich
    // eine ganze Reihe entsteht.
    //
    // Der Name sagt bewusst, WAS es tut, und nicht "Autoklicker": wiederholt wird nur, was man
    // selbst gedrueckt haelt, und genau so schnell wie in Vanilla.
    pub(crate) hold_use_repeat: HoldUseRepeat,
    // Darf ein Rechtsklick waehrend des Abbaus durchgehen? Vanilla verschluckt ihn
    // (MinecraftClient.doItemUse: `if (!interactionManager.isBreakingBlock())`) -- im
    // Crystal-PvP kostet das Kristalle, sobald ein Linksklick verrutscht.
    //
    // Standard AN, anders als bei snap_tap_enabled: dieses Verhalten aendert KEIN Paket. Der
    // Abbau laeuft weiter, es geht nur das Benutzen-Paket raus, das ein Vanilla-Client bei
    // gleicher Eingabe auch schickt -- nur eben spaeter. AUS ist trotzdem da, fuer den Fall,
    // dass man bewusst ganz auf Vanilla stehen will.
    pub(crate) use_while_mining: bool,
    // Werden Feuerwerksraketen beim Halten der Benutzen-Taste wiederholt? Eigener Schalter
    // neben hold_use_repeat: er gilt fuer Raketen statt der Stufe dort (Nutzerwunsch).
    pub(crate) hold_use_repeat_rockets: bool,
    // Rechtsklick nach dem Ausleeren eines Wasser-/Pulverschnee-Eimers zurueckhalten, solange man
    // brennt, bis der Server das Ausleeren bestaetigt hat (s. App::bucket_ack_wait).
    pub(crate) no_extinguish_dc_ghost: bool,
    // Die aktiven Skins/Resourcepacks, gestapelt: der ERSTE liegt oben und gewinnt, jeder
    // weitere fuellt nur auf, was die darueber nicht mitbringen (s. MinecraftInstallation::
    // resource_pack_dirs). Jeweils ein Unterordner-NAME unter skins_dir() (~/coral-client/skins),
    // NICHT der volle Pfad (s. skins_dir/list_available_skins). Leer = kein Pack, reines
    // Vanilla-Verhalten. Wie bei audio_output_device bewusst freie Strings statt Indizes, die
    // erst der naechste Start kennt. Gespeichert unter dem alten Schluessel "active_skin",
    // mit '|' getrennt (s. parse_skin_stack) -- eine alte Datei mit EINEM Pack liest sich so
    // unveraendert.
    pub(crate) active_skins: Vec<String>,
    // Reihenfolge der linken Spalte im Skin-Tab (verfuegbar, nicht gewaehlt): wer zuletzt aus
    // dem Stapel kam, steht oben (s. skin_tab::available_column). Gespeichert wie active_skins.
    pub(crate) skin_order: Vec<String>,
    // Je Pack, was darin ausgeschaltet ist: Dateien und Ordner relativ zu assets/minecraft (s.
    // assets::PackDir). Packs ohne Eintrag fehlen. Gespeichert als JSON-Objekt in einer Zeile.
    pub(crate) skin_exclusions: std::collections::BTreeMap<String, Vec<String>>,
    // Server-Packs: Platz des Server-Eintrags im Stapel, je Server-Adresse und je Pack-SHA-1
    // (s. server_packs::position_key/keys). Ausgeschaltetes steht wie bei eigenen Packs in
    // skin_exclusions, unter den Schluesseln aus server_packs::keys.
    pub(crate) server_pack_positions: std::collections::BTreeMap<String, usize>,
    // Zips im Skin-Ordner von selbst entpacken (s. skin_zips), solange der Skin-Tab offen ist.
    pub(crate) skin_auto_unzip: bool,
    // Einstellungen -> Sprache: je Bereich eine Sprache (Index in i18n::Language::ALL).
    // lang_ui ist die Oberflaeche dieses Clients, die anderen waehlen Minecrafts Sprachdatei
    // fuer Itemnamen, Effektnamen und Server-Texte (s. App::apply_languages). lang_chat ist
    // zugleich die Locale, die der Server erfaehrt.
    pub(crate) lang_ui: u8,
    pub(crate) lang_items: u8,
    pub(crate) lang_effects: u8,
    pub(crate) lang_chat: u8,
    // Latein mit Laengezeichen (ā ē ī ō ū) -- in der Oberflaeche und in den Item-, Block-,
    // Effekt- und Verzauberungsnamen (s. add_latin_macrons, i18n::set_latin_macrons).
    pub(crate) latin_macrons: bool,
    // Deutsche Oberflaeche mit englischen Lehnwoertern (Crosshair statt Fadenkreuz usw., s.
    // i18n::set_english_loanwords).
    pub(crate) english_loanwords: bool,
    // Farben der Oberflaeche (s. theme.rs), je Platz ein sRGB-Wert -- die des gewaehlten Themes.
    pub(crate) theme: theme::Colors,
    pub(crate) theme_choice: ThemeChoice,
    // Eigene Themes, in der Reihenfolge ihrer Kacheln.
    pub(crate) custom_themes: Vec<CustomTheme>,
    // Zuletzt offene Kategorie der Einstellungen (MenuScreen::settings_key); leer = noch nie
    // drin, dann oeffnet die Einrichtung.
    pub(crate) settings_category: String,
    // Scrollstand je Einstellungsseite (s. App::scroll_slots), als scroll_<name>=Zeile.
    pub(crate) settings_scrolls: std::collections::BTreeMap<String, usize>,
    // Online-Mode-Login (s. auth-Crate). msa_refresh_token haelt die Anmeldung ueber Neustarts
    // (wird beim Start gegen frische Tokens getauscht). account_name ist nur fuer die Anzeige
    // "Angemeldet als ..." zwischengespeichert.
    pub(crate) msa_refresh_token: String,
    pub(crate) account_name: String,
    // Minecraft-Access-Token + Profil-UUID (Hex ohne Bindestriche) der letzten Anmeldung --
    // damit man nach einem Neustart angemeldet bleibt, ohne Token/Device-Code erneut
    // einzugeben. ACHTUNG: Access-Tokens laufen nach ~24h ab; ist es dann tot, faellt der
    // naechste Connect auf einen klaren Login-Fehler und man meldet sich neu an.
    pub(crate) mc_access_token: String,
    pub(crate) account_uuid: String,
}

// Azure-Application-(Client-)ID der eigenen App dieses Clients (App-Registrierung "coral-client",
// nur persoenliche Microsoft-Konten, oeffentliche Clientflows fuer den Device-Code-Flow). Die
// Minecraft-Anmeldung (login_with_xbox) nimmt sie an, seit Mojang sie freigegeben hat. Jede
// neue Anmeldung laeuft ueber diese ID; gespeicherte Konten behalten die ihres Refresh-Tokens.
pub(crate) const AZURE_CLIENT_ID: &str = "4e36a581-063a-4bbf-98bb-77b91766d32c";

// Name der Config, mit der dieser Client laeuft -- beim Start in main() festgelegt (--config
// vom Launcher, sonst die zuletzt gewaehlte, s. assets::startup_config), danach im Spiel
// umschaltbar (s. App::switch_config).
pub(crate) static ACTIVE_CONFIG: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

impl Settings {
    // Maximaler Schneeradius nie kleiner als der Schneeradius: ein groesserer Schneeradius schiebt
    // ihn mit, kleiner gezogen haelt er am Schneeradius an.
    pub(crate) fn couple_snow_radius(&mut self) {
        self.weather_snow_radius_max = self.weather_snow_radius_max.max(self.weather_snow_radius);
    }

    // Partikel-Stufe und -Anpassung der eigenen (true) oder der anderen Partikel.
    pub(crate) fn particles_mode_of(&mut self, own: bool) -> &mut u8 {
        if own { &mut self.particles_mode_own } else { &mut self.particles_mode }
    }

    pub(crate) const fn particle_tuning_of(&self, own: bool) -> &[particles::ParticleTuning; particles::KINDS.len()] {
        if own { &self.particle_tuning_own } else { &self.particle_tuning }
    }

    pub(crate) fn particle_tuning_of_mut(&mut self, own: bool) -> &mut [particles::ParticleTuning; particles::KINDS.len()] {
        if own { &mut self.particle_tuning_own } else { &mut self.particle_tuning }
    }
}

pub(crate) fn active_config() -> String {
    let name = ACTIVE_CONFIG.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    if name.is_empty() {
        assets::DEFAULT_CONFIG.to_string()
    } else {
        name
    }
}

pub(crate) fn set_active_config(name: &str) {
    *ACTIVE_CONFIG.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = name.to_string();
}

// Einstellungen der aktiven Config: ~/coral-client/configs/<Name>.conf (Windows:
// %USERPROFILE%\coral-client\configs\<Name>.conf). Der private Teil (s. is_private_setting)
// ist fuer alle Configs derselbe: ~/coral-client/private-config.conf, bleibt beim Wechseln.
pub(crate) fn settings_path() -> Option<std::path::PathBuf> {
    assets::config_path(&active_config())
}

pub(crate) fn private_settings_path() -> Option<std::path::PathBuf> {
    assets::private_config_path(&active_config())
}

// Wie oft nachgesehen wird, ob die Config von aussen geaendert wurde: ein stat je Datei, kurz
// genug, dass ein Wert aus dem Editor beim Wechsel ins Spiel schon gilt.
pub(crate) const CONFIG_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

// Pfad und Aenderungszeiten von oeffentlicher und privater Config. Mit Pfad, damit ein
// Config-Wechsel oder Umbenennen nicht als Aenderung von aussen gilt.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ConfigStamp {
    pub(crate) path: Option<std::path::PathBuf>,
    pub(crate) modified: [Option<std::time::SystemTime>; 2],
}

pub(crate) fn config_stamp() -> ConfigStamp {
    let modified = |p: Option<std::path::PathBuf>| p.and_then(|p| std::fs::metadata(p).ok()?.modified().ok());
    ConfigStamp { path: settings_path(), modified: [modified(settings_path()), modified(private_settings_path())] }
}

// Schluessel, die nur zu diesem Rechner und Spieler passen und deshalb nicht in die
// weitergebbare Config gehoeren: Konto, eigene Skin-Packs, Scanout-Timing (je Monitor
// eingemessen), Bildrate des Monitors, Audiogeraete, Thread-Zahlen der CPU und wo man im
// Menue zuletzt stand.
pub(crate) fn is_private_setting(key: &str) -> bool {
    matches!(
        key,
        "msa_refresh_token"
            | "account_name"
            | "mc_access_token"
            | "account_uuid"
            | "active_skin"
            | "skin_order"
            | "tear_position"
            | "tear_timing_us"
            | "tear_reserve_us"
            | "fps_limit_hz"
            | "audio_output_device"
            | "audio_buffer_frames"
            | "audio_auto_optimize"
            | "voice_mic_device"
            | "chunk_update_threads"
            | "chunk_saver_threads"
            | "settings_category"
    ) || key.starts_with("scroll_")
}

// Gespeicherter Text in (Config, privater Teil) aufteilen.
pub(crate) fn split_private_settings(text: &str) -> (String, String) {
    let (mut oeffentlich, mut privat) = (String::new(), String::new());
    for zeile in text.lines() {
        let schluessel = zeile.split_once('=').map_or("", |(k, _)| k.trim());
        let ziel = if is_private_setting(schluessel) { &mut privat } else { &mut oeffentlich };
        ziel.push_str(zeile);
        ziel.push('\n');
    }
    (oeffentlich, privat)
}

// Was fuer alle Configs gleich ist (Server-Liste, Chat-Verlauf, eigene Klaenge, Hintergrundbild),
// liegt direkt in ~/.config/coral-client.
pub(crate) fn shared_config_dir() -> Option<std::path::PathBuf> {
    assets::config_dir()
}
