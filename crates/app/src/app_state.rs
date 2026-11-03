// Der Zustand der App.

use crate::*;

pub(crate) struct App {
    pub(crate) gpu: Option<GpuState>,
    // Arbeitszeit (Vertex-Aufbau, Encoding, Submit) und Wartezeit (get_current_texture())
    // getrennt gefuehrt -- bei Fifo ist die Wartezeit der eigentlich interessante Wert,
    // bei Immediate/Mailbox sollte sie nahe null liegen. s. FrameTiming.
    pub(crate) frame_stats: FrameStats,
    // Seit wann die Schleife schlaeft (Ende von about_to_wait), s. FrameStats.
    pub(crate) loop_idle_since: Option<Instant>,
    // s. EntityUpdateStats-Kommentar: F3-Anzeige, wie viele Entity-Positions-Pakete
    // tatsaechlich pro Sekunde ankommen.
    pub(crate) entity_update_stats: EntityUpdateStats,
    // Lokale Minecraft-Installation: harte Voraussetzung, beim Start geprueft.
    // Liefert Font und (spaeter) Block-Texturen -- deshalb einmal oeffnen und behalten.
    pub(crate) installation: assets::MinecraftInstallation,
    // Himmel aus den aktiven Packs (OptiFine-Format, s. custom_sky); leer ohne solche Packs.
    pub(crate) custom_sky: custom_sky::CustomSky,
    // Biomnamen nach ID (aus der Registry des Servers), fuer biomes= im Pack-Himmel.
    pub(crate) biome_names: Vec<String>,
    // Block-Texturatlas (einmal beim Start gebaut). mips/size gehen beim GPU-Setup an den
    // BlockRenderer, block_uvs versorgt den Mesher. Die weisse Kachel des BLOCK-Atlas wird
    // hier bewusst nicht gefuehrt: einfarbige Flaechen entstehen im Entity-Durchgang und
    // brauchen die weisse Kachel des ENTITY-Atlas (s. EntityAtlas::white_rect).
    pub(crate) atlas_mips: Vec<Vec<u8>>,
    pub(crate) atlas_size: u32,
    // Wasser-Textur-Animation (s. render::BlockRenderer::upload_atlas_tiles-Kommentar):
    // alle Frames + die Zielkachel im Atlas, plus Laufzeitzustand (aktueller Frame-Index,
    // wann zuletzt gewechselt). frametime=2 Ticks (water_still.png.mcmeta) bei 20 TPS =
    // 100ms zwischen Frames -- reale Zeit statt Physik-Ticks, das ist reine Optik.
    pub(crate) water_animation_frames: Vec<Vec<u8>>,
    pub(crate) water_tile_origin: Option<(u32, u32)>,
    // Dasselbe fuer die FLIESS-Kachel (water_flow, s. assets::BlockAtlas). Eigene Frames und
    // eigener Zielbereich im Atlas, aber derselbe Takt -- beide Wassertexturen laufen in
    // Vanilla mit frametime=2 und muessen synchron bleiben, sonst flimmert der Uebergang
    // zwischen ruhiger und fliessender Zelle.
    pub(crate) water_flow_animation_frames: Vec<Vec<u8>>,
    pub(crate) water_flow_tile_origin: Option<(u32, u32)>,
    // Kantenlaenge einer Wasser-Frame-Kachel (== atlas.tile_size, s. dortigen Kommentar) --
    // frueher fest 16, jetzt dynamisch (HD-Resourcepacks).
    pub(crate) water_tile_size: u32,
    pub(crate) water_anim_frame: usize,
    pub(crate) water_anim_last_switch: Instant,
    // Alle uebrigen animierten Blocktexturen (s. assets::TileAnimation), gerechnet in einem
    // eigenen Thread (s. block_anim.rs). Die Uhr laeuft ab Atlas-Bau in Spielticks (20/s), wie
    // Vanillas Sprite-Animation.
    pub(crate) block_animations: block_anim::BlockAnimations,
    // Vertices der Entities im letzten Frame (s. build_entity_vertices).
    pub(crate) entity_vertex_hint: usize,
    pub(crate) block_anim_start: Instant,
    // Zuletzt hochgeladener Stand je Animation des UI-Atlas (s. UiAtlas::animations).
    pub(crate) ui_anim_last: Vec<Option<assets::AnimationState>>,
    // Deckkraft der Feuerbloecke (Modul "Dynamisches Feuer"), 0..1, s. tick_fire_alpha.
    pub(crate) fire_alpha: f32,
    // 2D-Oberflaeche: der UI-Atlas (GUI-Sheets + flache Item-Sprites, s. assets::ui_atlas)
    // und die daraus aufgeloesten Item-Icons. Beides wird wie der Blockatlas beim Start und
    // bei jedem Resourcepack-Wechsel neu gebaut.
    pub(crate) ui_atlas: assets::UiAtlas,
    pub(crate) item_icons: ItemIcons,
    // Entity-Texturatlas + die einmal gebauten Modelle (s. WorldAssets::build).
    pub(crate) entity_atlas: assets::EntityAtlas,
    // Partikel-Sprites (s. assets::ParticleAtlas) -- gehalten, weil die GPU erst nach der App
    // entsteht und den Atlas dann hochlaedt.
    pub(crate) particle_atlas: assets::ParticleAtlas,
    // Vanillas ParticleManager (s. particles.rs) und der wiederverwendete Vertexpuffer.
    pub(crate) particles: particles::Particles,
    pub(crate) particle_vertices: Vec<render::ParticleVertex>,
    // Regen und Schnee (s. precipitation) und ihr wiederverwendeter Vertexpuffer.
    pub(crate) precipitation: precipitation::Precipitation,
    pub(crate) weather_vertices: Vec<render::WeatherVertex>,
    // Regen-/Schnee-Texturen (s. WeatherTexture) und welche Wahl gerade auf der GPU liegt:
    // Some(true) = mit Vanilla-Ersatz fuer leere Pack-Bilder, None = noch nichts/neu laden.
    pub(crate) weather_textures: [WeatherTexture; 2],
    pub(crate) weather_textures_uploaded: Option<bool>,
    // Eingerastete Breiten wechselnder HUD-Texte (s. latched_width): je Schluessel die groesste
    // je gemessene Breite. RefCell, weil das HUD in &self gebaut wird.
    pub(crate) hud_latched_widths: std::cell::RefCell<std::collections::HashMap<&'static str, f32>>,
    // Items in den Haenden anderer (Texturplaetze + aufgeloeste Modelle, s. held_item).
    pub(crate) held_items: held_item::HeldItems,
    // Letzter Inventar-Klick (Slot + Taste) und sein Zeitpunkt -- nur fuer die
    // Doppelklick-Erkennung (s. INVENTORY_DOUBLE_CLICK).
    pub(crate) last_inventory_click: Option<(usize, i8)>,
    pub(crate) last_inventory_click_at: Instant,
    // Laufendes Verteilen-Ziehen (s. inventory_click/-_release). Some, sobald eine Taste mit
    // vollem Zeiger gedrueckt wurde -- ob daraus ein Ziehen oder ein gewoehnlicher Klick
    // wird, entscheidet erst das Loslassen.
    pub(crate) inventory_drag: Option<InventoryDrag>,
    pub(crate) quick_craft_pending: Option<QuickCraftPending>,
    // Haltezustand des Stapel-Verschieben-Modifikators (s. KeyBindAction::InventoryMoveStack).
    // Ein eigenes Feld wie sneak_held/sprint_held -- ohne das faellt action_active auf
    // `_ => false` zurueck und der Modifikator ist dauerhaft aus.
    pub(crate) inventory_move_stack_held: bool,
    // Tab-Liste sichtbar (s. KeyBindAction::PlayerList, build_player_list).
    pub(crate) player_list_held: bool,
    pub(crate) entity_models: Vec<(&'static str, world::Model, world::entity_model::ModelKind)>,
    pub(crate) player_wide: world::Model,
    pub(crate) player_slim: world::Model,
    // PlayerCapeModel -- reine Daten, einmal gebaut (s. world::cape_model).
    pub(crate) cape_model: world::Model,
    // Zauberschrift fuer die Zierzeilen des Zaubertischs (s. App::push_alt_text).
    pub(crate) alt_font: assets::AltFontGlyphs,
    // Das Buch im Zaubertisch-Bildschirm: Modell und Blaetter-Animation (s. BookAnim).
    pub(crate) book_model: world::Model,
    pub(crate) book_anim: BookAnim,
    // Bett-BlockEntity-Modelle (s. world::entity_model::bed_head_model).
    pub(crate) bed_head_model: world::Model,
    pub(crate) bed_foot_model: world::Model,
    // Ein Ruestungsmodell je Slot, nach ArmorSlot::index() (s. entity_model::armor_model).
    pub(crate) armor_models: [world::Model; 4],
    pub(crate) shield: ShieldRender,
    // Inventar-/Containerzustand aus den Server-Paketen (s. inventory.rs).
    pub(crate) inventory: Inventory,
    // Rezeptbuch neben Inventar und Werkbank (s. recipe_book.rs).
    pub(crate) recipe_book: recipe_book::RecipeBook,
    pub(crate) block_uvs: std::sync::Arc<world::BlockUvs>,
    // Kollisions-/Render-Boxen pro State (Stufen, Zaeune, ... statt Vollwuerfel), s.
    // assets::shapes. Versorgt Mesher UND Physik (dieselbe Form fuer Optik und Kollision).
    pub(crate) block_shapes: std::sync::Arc<world::BlockShapes>,
    // Welche States Kreuz-Bloecke sind (Zuckerrohr, Gras, Blumen, ...) -- eigener
    // Zeichenpfad im Mesher statt block_shapes' Vollwuerfel-Fallback, s. world::cross_shapes.
    pub(crate) cross_blocks: std::sync::Arc<world::CrossBlocks>,
    // Zusaetzliche, unrotierte Elemente eines Kreuz-Modells (z.B. Hitbox-Deko-Ebenen eines
    // PvP-Cobweb-Packs) -- eigene, von block_shapes GETRENNTE Tabelle, s. assets::shapes::
    // build_cross_extras-Kommentar (block_shapes muss fuer Kreuz-States leer bleiben, sonst
    // wuerden diese randvollen Boxen faelschlich Nachbarflaechen anderer Bloecke verdecken).
    pub(crate) cross_extra_shapes: std::sync::Arc<world::BlockShapes>,
    // Rohe Colormap-Bilder (einmal geladen, versionsunabhaengig von der Verbindung).
    // biome_palette ist die daraus PRO VERBINDUNG gesampelte Tabelle (s. drain_events,
    // ClientEvent::BiomeRegistry) -- deshalb getrennt und ein Arc, damit der Mesher-Worker
    // wie bei block_uvs/block_shapes nur einen Klon braucht.
    pub(crate) grass_colormap: Option<assets::Colormap>,
    pub(crate) foliage_colormap: Option<assets::Colormap>,
    // Dritte Colormap (seit 1.21.2) fuer Falllaub-Bodenbedeckung (leaf_litter) -- eigene
    // Farbverteilung, nicht dieselbe wie foliage.png (Laub-Baelle), s. world::BiomeColors.
    pub(crate) dry_foliage_colormap: Option<assets::Colormap>,
    pub(crate) biome_palette: std::sync::Arc<world::BiomePalette>,
    // Timer-basierter Frame-Cap (statt Poll-Busylooping): haelt Immediate-Present-Mode
    // fuer niedrige Latenz, spart aber Strom, indem der Prozess zwischen Frames schlaeft.
    // Aus der Refresh-Rate des Monitors; der wirksame Wert kommt aus effective_frame_interval().
    pub(crate) monitor_frame_interval: Duration,
    pub(crate) next_frame_time: Instant,
    // Laeuft die Physik gerade? (World-Modus UND eigene Spalte geladen, s. update_movement.)
    pub(crate) ticking: bool,
    // Wann der naechste Physik-Tick faellig ist -- None, wenn gerade nicht getickt wird
    // (Menue, oder eigene Chunk-Spalte noch nicht geladen).
    //
    // Die Fensterschleife wacht auch fuer diesen Termin auf, nicht nur fuer den Frame-Termin
    // (s. next_wake). Ohne das feuerte ein faelliger Tick erst beim naechsten Frame-Aufwachen
    // und damit bis zu ein Frame-Intervall zu spaet -- ein Verzug, der direkt auf jedem
    // Bewegungspaket landet, weil die Pakete IM Tick rausgehen.
    pub(crate) tick_deadline: Option<Instant>,
    // Modul Nachhaltigkeit: das Weltbild hinter dem Pausemenue ist gezeichnet und bleibt
    // stehen, bis man zurueck in der Welt ist (s. sustain_pause).
    pub(crate) menu_world_frozen: bool,
    // Configs-Seite: "Löschen" einmal geklickt, der zweite Klick loescht wirklich.
    pub(crate) config_delete_armed: bool,
    // Lichtstaerken-Seite: Suchtext und erste sichtbare Zeile der Trefferliste.
    pub(crate) light_search: String,
    pub(crate) light_scroll: smooth_scroll::SmoothScroll,
    // Erste sichtbare Zeile der Partikel-Anpassung.
    pub(crate) particle_scroll: smooth_scroll::SmoothScroll,
    // Welche Partikel die Anpassung gerade zeigt: true = die eigenen (s. ButtonAction::OpenParticleSettings).
    pub(crate) particle_screen_own: bool,
    // Skin-Tab: Scrollstand beider Spalten (links die Liste, "Kein Pack" zaehlt mit; rechts der
    // gewaehlte Stapel), in Kacheln.
    pub(crate) skin_scroll: [smooth_scroll::SmoothScroll; 2],
    pub(crate) start_time: Instant,
    // Direkter evdev-Zugriff auf die Maus (X11-Bypass). None = kein Zugriff moeglich,
    // dann faellt device_event() auf den ueblichen winit/X11-Weg zurueck.
    pub(crate) raw_mouse: Option<raw_input::RawMouse>,
    // Direkter evdev-Zugriff auf die Tastatur (X11-Bypass), analog zur Maus. None = kein
    // Zugriff -> Spielsteuerung laeuft ueber winits WindowEvent::KeyboardInput. Ist er aktiv,
    // ist er im World-Modus die ALLEINIGE Quelle fuer Spieltasten (s. window_event/Guard).
    pub(crate) raw_keyboard: Option<raw_input::RawKeyboard>,
    // Loest die Feststell-Sperre im Spiel (s. Settings::caps_lock_blocked).
    pub(crate) caps_lock: caps_lock::CapsLockGuard,
    // Wann die Sperre nach einem Druck noch einmal geloest wird (s. track_bind).
    pub(crate) caps_lock_recheck: Option<Instant>,
    // Meldet Eingabegeraete, die im laufenden Betrieb angesteckt und uebernommen wurden (s.
    // raw_input::DeviceWatcher). None = kein evdev-Zugriff.
    pub(crate) input_watcher: Option<raw_input::DeviceWatcher>,
    // Modul "input-log" (s. input_log.rs). Immer vorhanden, aber nur aktiv, wenn im
    // Modul-Menue eingeschaltet -- dann schreibt es die per Raw-Input erkannten Spiel-
    // Eingaben mit Zeitstempel in eine lokale Datei.
    pub(crate) input_log: input_log::InputLog,
    // coral-record: Kamera, Maus und lokaler Zustand fuer die Aufnahme (s. app/recording.rs).
    pub(crate) recorder: app::AppRecorder,
    // Liste der Aufnahmen im Connect-Menue und die laufende Wiedergabe (s. app/replay.rs).
    pub(crate) recordings_open: bool,
    pub(crate) recordings: Vec<app::RecordingEntry>,
    pub(crate) replay: Option<app::Replay>,
    // Debug-Modul "Autoklicker" (s. tick_autoclicker).
    pub(crate) autoclick: Autoclick,
    // Modul Session-Info: zaehlt immer, auch ausgeschaltet (s. session_info.rs).
    pub(crate) session: session_info::Session,
    // Todesanimation (Video > Tod): Ticks seit dem Tod je sterbendem Lebewesen (Vanillas
    // deathTime, bis der Server es entfernt) und die fliegenden Teile der Explosion.
    pub(crate) dying: std::collections::HashMap<i32, u32>,
    pub(crate) death_bursts: Vec<death_burst::Burst>,
    // Hintergrundbild des Startbildschirms: Laden im Hintergrund (s. wallpaper.rs) und wann
    // zuletzt nachgesehen wurde, ob sich der Desktop-Hintergrund geaendert hat.
    pub(crate) wallpaper_loader: wallpaper::Loader,
    pub(crate) wallpaper_checked_at: Instant,
    // Laden des gespeicherten letzten Weltblicks (s. poll_last_view).
    pub(crate) last_view_requested: bool,
    pub(crate) last_view_rx: Option<Receiver<Option<wallpaper::Decoded>>>,
    // Fuer welchen Servereintrag GpuState::connect_view gilt, und ein laufendes Laden dafuer.
    pub(crate) connect_view_for: Option<String>,
    pub(crate) connect_view_rx: Option<(String, Receiver<Option<wallpaper::Decoded>>)>,
    // "Erster Moment" (s. App::tick_connect_view_capture): schon aufgenommen, und seit wann
    // Chunks da sind. Beides je Verbindung.
    pub(crate) connect_view_taken: bool,
    pub(crate) connect_view_first_at: Option<Instant>,
    // Launcher-Hintergrundbild (s. hero_scene.rs): seit wann die Szene in der Welt steht, und ob
    // fuer diesen Start nichts mehr zu tun ist.
    pub(crate) hero_building: Option<Instant>,
    pub(crate) hero_done: bool,
    // Sichtfeld der Szene, solange sie steht (statt der Einstellung, s. draw_world_frame).
    pub(crate) hero_fov: Option<f32>,
    // Fenster, das der Ladebildschirm schon geoeffnet hat (s. loading_screen); resumed nimmt es
    // statt ein neues anzulegen.
    pub(crate) startup_window: Option<(Arc<Window>, winit::monitor::VideoModeHandle)>,
    pub(crate) hero_render_distance: Option<u32>,
    // No Tool Break: wann zuletzt benachrichtigt wurde (s. block_mining_guarded).
    pub(crate) tool_break_notified_at: Option<Instant>,
    pub(crate) server_address: String,
    // Online-Mode-Anmeldung (s. auth-Crate). Some = angemeldet, wird bei start_connect an
    // net::join durchgereicht (echter Verschluesselungs-Handshake). None = Offline-Mode
    // (unveraendertes bisheriges Verhalten fuer den lokalen Test-Server).
    pub(crate) credentials: Option<auth::Credentials>,
    // Signierer fuer Chatnachrichten. Some, sobald der Profilschluessel geholt und die
    // Sitzung beim Server angemeldet ist (s. request_profile_key) -- ohne ihn gehen
    // Nachrichten unsigniert raus und Server mit enforce-secure-profile lehnen sie ab.
    pub(crate) chat_signer: Option<net::ChatSigner>,
    // Ergebnisse der Hintergrund-Auth-Tasks (Device-Code-Anzeige, fertige Anmeldung, Fehler),
    // analog zu events_tx/rx fuer die Netz-Events -- der tokio-Task darf die App nicht direkt
    // anfassen.
    pub(crate) auth_tx: Sender<AuthEvent>,
    pub(crate) auth_rx: Receiver<AuthEvent>,
    // Laeuft gerade ein Login-Vorgang? Verhindert doppelte Device-Code-Anfragen bei
    // mehrfachem Klick.
    pub(crate) login_busy: bool,
    // Der Microsoft-Login, der auf die Anmeldung im Browser wartet (Browser-Login bis zu 10 min,
    // Device-Code bis zu ~15 min). Ein neuer Anmeldeversuch bricht ihn ab, s. cancel_device_login.
    pub(crate) device_login: Option<tokio::task::JoinHandle<()>>,
    // Eingabefeld auf dem Account-Screen (Token ODER Refresh-Token): Some = Feld
    // sichtbar+fokussiert (dann gehen Tastatur/Einfuegen dorthin statt in die Adresse),
    // None = ausgeblendet. Immer nur eines gleichzeitig offen.
    pub(crate) input_field: Option<(InputKind, String)>,
    // Gespeicherte Server (Liste rechts auf dem Connect-Screen), aus servers.conf geladen und
    // bei jeder Aenderung sofort dorthin zurueckgeschrieben -- wie bei den Einstellungen gibt
    // es bewusst keinen Speichern-Button.
    pub(crate) servers: Vec<ServerEntry>,
    // Rechtsklick-Menue einer Karte und eine gedrueckte/gezogene Karte (s. ServerDrag).
    pub(crate) server_context: Option<ServerContextMenu>,
    pub(crate) server_drag: Option<ServerDrag>,
    // Scrollstand der Server-Karten (Mausrad ueber der Liste), in Karten.
    pub(crate) server_scroll: smooth_scroll::SmoothScroll,
    // Griff der Bildlaufleiste links neben den Karten gefasst: Abstand des Zeigers zur
    // Oberkante des Griffs (s. App::server_bar_press).
    pub(crate) server_bar_grab: Option<f32>,
    // Suche wie in osu!: einfach lostippen, dann steht das Feld an der Stelle der ersten Karte
    // und nur passende Karten bleiben (Name, Adresse, MOTD). `server_search_applied` ist der
    // Suchtext, auf den der Scrollstand zuletzt zurueckgesetzt wurde.
    pub(crate) server_search: Option<text_field::TextField>,
    pub(crate) server_search_applied: String,
    pub(crate) ui_anim_at: Instant,
    // Quick Connect: Fenster mit Adresse und "Los" (Leiste unten). Das Feld ist beim Oeffnen
    // leer; erst "Los" uebernimmt es in server_address. Ziehen markiert, Doppelklick nimmt das
    // Wort (wie im Server-Fenster, s. ServerEdit).
    pub(crate) quick_connect_open: bool,
    // Namensfenster fuer ein neues Theme ("+"-Kachel) und die Rueckfrage vor dem Loeschen eines
    // eigenen (Index in custom_themes, per Rechtsklick).
    pub(crate) theme_name_dialog: Option<ThemeNameDialog>,
    pub(crate) theme_delete: Option<usize>,
    // Meldung, mit der der Server uns rausgeworfen hat (oder warum die Verbindung abriss) --
    // als Fenster ueber der Server-Liste, bis man es schliesst. Beim eigenen Disconnect None.
    pub(crate) disconnect_popup: Option<String>,
    // Server-Dialoge (s. dialog_screen.rs): der offene Dialog, das Warten auf die Antwort des
    // Servers, der Verhaltenskodex aus der Konfiguration, die Links des Servers und die
    // Dialog-Registry, auf die Show Dialog im Spiel verweisen kann.
    pub(crate) dialog: Option<dialog_screen::OpenDialog>,
    pub(crate) dialog_waiting: Option<Instant>,
    // Link, dessen Bestaetigungsfenster gerade offen ist: "Öffnen" darf genau ihn oeffnen.
    pub(crate) link_pending: Option<String>,
    pub(crate) code_of_conduct: Option<dialog_screen::CodeOfConduct>,
    pub(crate) server_links: Vec<protocol::packets::ServerLink>,
    pub(crate) dialog_registry: Vec<(String, Option<protocol::nbt::NbtValue>)>,
    // Teams-Modul: zuletzt ins Log geschriebene Zeile je Spieler (s. update_team_facts).
    pub(crate) team_color_log: std::collections::HashMap<[u8; 16], String>,
    // Merkmale je Spieler fuers Teams-Modul, einmal pro Tick (s. update_team_facts) -- das
    // Zeichnen der Hitboxen liest nur hier.
    pub(crate) team_facts: std::collections::HashMap<[u8; 16], teams::Facts>,
    // Zuletzt erkannte Namensfarbe je Spieler: ein Namensschild ist nicht immer zu sehen
    // (kurz entfernt, neu gesetzt), das Team wechselt dabei nicht. Gilt bis zum Weltwechsel.
    pub(crate) team_color_memory: std::collections::HashMap<[u8; 16], u32>,
    pub(crate) quick_field: text_field::TextField,
    pub(crate) quick_dragging: bool,
    pub(crate) quick_last_press: Option<(Instant, (f32, f32))>,
    // Kontenliste unter dem Konto-Knopf der Leiste oben.
    pub(crate) account_menu_open: bool,
    // Acc-Manager: Fenster mit allen Konten, Anmelden und Abmelden.
    pub(crate) account_manager_open: bool,
    // Status-Ping je Server-Adresse (s. server_status), Ergebnisse kommen ueber den Kanal.
    pub(crate) server_status: std::collections::HashMap<String, server_status::Entry>,
    pub(crate) server_status_tx: std::sync::mpsc::Sender<(String, Option<String>)>,
    pub(crate) server_status_rx: std::sync::mpsc::Receiver<(String, Option<String>)>,
    pub(crate) next_icon_slot: usize,
    // Liegt das Bild fuer Server ohne Icon schon im Atlas (s. UNKNOWN_SERVER_ICON_SLOT)?
    pub(crate) unknown_server_icon: bool,
    // Offenes Fenster "Neuer Eintrag"/"Eintrag bearbeiten" (s. build_server_dialog). Some
    // liegt ueber der Seite und uebernimmt die Tastatureingabe.
    pub(crate) server_edit: Option<ServerEdit>,
    // Gespeicherte Konten des Acc-Managers (s. accounts.rs), dazu je Konto (Schluessel:
    // accounts::key) das Ergebnis der letzten Pruefung und wann sie begann bzw. endete.
    pub(crate) accounts: accounts::Store,
    pub(crate) account_checks: std::collections::HashMap<String, (accounts::Check, Instant)>,
    // Rechtsklick-Menue im Acc-Manager: Konto (Index in accounts.list) und Klickpunkt.
    pub(crate) account_context: Option<(usize, (f32, f32))>,
    // Token-Export auskommentiert, vorerst abgeschaltet.
    // pub(crate) token_export: Option<TokenExport>,
    // Chat (urspruenglich fuers Debugging gedacht: Grim-Meldungen/Server-Antworten
    // direkt im Client sehen, ohne die Server-Konsole zu beobachten). Some(text) waehrend
    // der Eingabe (geoeffnet per T, s. window_event) -- blockiert waehrenddessen
    // Bewegungstasten/Maussicht, genau wie Vanillas Chat-Feld.
    pub(crate) chat_input: Option<chat_input::ChatInput>,
    // Gesendete Nachrichten. Liegt NEBEN dem Eingabefeld, weil der Verlauf das Schliessen des
    // Chats ueberleben muss -- man will die letzte Nachricht auch nach dem Zumachen noch mit
    // Pfeil-oben heraufholen.
    pub(crate) chat_history: Vec<String>,
    // Letzte empfangene System-Chat-Nachrichten (CB_PLAY_SYSTEM_CHAT), aelteste zuerst,
    // gedeckelt auf CHAT_MAX_MESSAGES.
    // Die Chat-Zeilen MIT ihren Farben (s. net::ClientEvent::Chat). Frueher stand hier ein
    // flachgeklopfter String -- damit war jede Nachricht gleich grau, egal was der Server
    // gefaerbt hatte.
    pub(crate) chat_messages: std::collections::VecDeque<Vec<protocol::nbt::TextSpan>>,
    // Wann jede Nachricht in chat_messages ankam (gleich lang, gleiche Reihenfolge) -- fuers
    // Ausblenden im geschlossenen Chat (Settings::chat_fade_secs).
    pub(crate) chat_times: std::collections::VecDeque<Instant>,
    // Deckel-Animation der Shulkerkisten und Truhen, nur fuer die, die gerade auf/zu gehen oder offen
    // stehen (s. tick_block_lids). Geschlossene fehlen hier und werden zu gezeichnet.
    pub(crate) block_lids: std::collections::HashMap<(i32, i32, i32), BlockLid>,
    // Zaehlt jede Aenderung an chat_messages -- Schluessel des Umbruch-Speichers.
    pub(crate) chat_generation: u64,
    // Gesetzter Verlauf fuer den offenen Chat (s. chat_wrapped_all).
    pub(crate) chat_wrap_cache: std::cell::RefCell<Option<((u64, u32, u32, u8), std::rc::Rc<Vec<ChatLine>>)>>,
    // Wie viele Zeilen der offene Chat vom Ende weg zurueckgeblaettert ist (0 = neueste unten).
    pub(crate) chat_scroll: usize,
    // chat_scroll gleitend nachgezogen (s. tick_ui_anim) -- gezeichnet wird damit.
    pub(crate) chat_scroll_glide: smooth_scroll::SmoothScroll,
    // Um so viele Zeilen steht der geschlossene Chat noch zu tief: jede neue Nachricht legt ihre
    // Zeilen DRAUF, der Wert laeuft gleitend gegen 0 (s. tick_ui_anim). Kommt die zweite
    // Nachricht, waehrend die erste noch hochgleitet, geht es von der aktuellen Lage aus
    // weiter -- nichts springt.
    pub(crate) chat_rise: f32,
    // Dasselbe fuer den offenen Chat (andere Breite, also andere Zeilenzahl).
    pub(crate) chat_rise_open: f32,
    // Bedrock-Chat: Zeilen aller Nachrichten bisher, in der Breite des Chats im Spiel (je Tab).
    // Daran haengen die festen Plaetze der Zeilen, auch wenn oben welche ausblenden.
    pub(crate) chat_lines_total: u64,
    // Bedrock-Chat: (erste gezeigte Zeile, Ende) wie das letzte Bild sie wollte, und derselbe
    // Stand gleitend (s. tick_ui_anim). None = beim naechsten Mal ohne Gleiten uebernehmen.
    pub(crate) chat_bedrock_target: std::cell::Cell<(f32, f32)>,
    pub(crate) chat_bedrock_anim: Option<(f32, f32)>,
    // Der Anfang, den das letzte Bild wollte -- um zu trennen, wie viel davon neue Nachrichten
    // weitergeschoben haben (springt) und wie viel ausgeblendete (gleitet).
    pub(crate) chat_bedrock_last_start: f32,
    // Zeilenstand des offenen Chats im letzten Bild: Index (ueber den ganzen Verlauf, auch
    // schon verworfene Nachrichten mitgezaehlt) der Zeile hinter der untersten sichtbaren,
    // und die Zeilenhoehe. Daran haengt die mitwandernde Auswahl (s. TextSelection::chat_anchor).
    pub(crate) chat_view: std::cell::Cell<Option<(usize, f32)>>,
    // Die Dimensionsarten des Servers in Registrierungsreihenfolge (s.
    // ClientEvent::DimensionRegistry) -- daraus kommt die Welt-Untergrenze.
    pub(crate) dimension_types: Vec<protocol::packets::DimensionInfo>,
    // Wie viele Zeilen die schon verworfenen Nachrichten hatten (s. push_chat_message).
    pub(crate) chat_dropped_lines: usize,
    // Verlaufsliste neben der Eingabe: wie weit zurueckgeblaettert, und wo ihre Eintraege
    // zuletzt standen (fuer Klicks, s. chat_click).
    pub(crate) chat_history_scroll: usize,
    pub(crate) chat_history_glide: smooth_scroll::SmoothScroll,
    pub(crate) chat_history_hits: std::cell::RefCell<Vec<(Rect, String)>>,
    // Klickflaechen der Links im zuletzt gezeichneten offenen Chat (s. chat_click).
    pub(crate) chat_link_hits: std::cell::RefCell<Vec<(Rect, ChatHit)>>,
    // Laufende oder stehende Textauswahl (Maus ziehen, s. TextSelection).
    pub(crate) text_selection: Option<TextSelection>,
    // Letzter Linksklick (Zeit, Ort, wievielter in Folge) -- fuer Doppel-/Dreifachklick.
    pub(crate) last_left_click: Option<(Instant, (f32, f32), u8)>,
    // EIN Zwischenablage-Objekt fuer die ganze Laufzeit: unter X11 gehoert der Inhalt dem
    // Programm, das ihn gesetzt hat, und verschwindet mit dem letzten Clipboard-Objekt.
    // Ein kurzlebiges je Kopiervorgang haette die Auswahl sofort wieder verloren.
    pub(crate) clipboard: Option<arboard::Clipboard>,
    // Actionbar-Text ueber der Hotbar (Set Action Bar Text / System-Chat-Overlay) + Zeitpunkt
    // des Empfangs. Verschwindet nach ACTION_BAR_DURATION (wie Vanilla ~3s).
    // Der Actionbar-Text MIT Farben (s. net::ClientEvent::ActionBar). Server faerben hier
    // Warnungen und Zustandsanzeigen -- einfarbig waere die Haelfte der Auskunft weg.
    pub(crate) action_bar: Option<(Vec<protocol::nbt::TextSpan>, Instant)>,
    // Titel/Untertitel (Bildschirmmitte) + Anzeigezeiten (Ticks, aus Set Titles Animation) und
    // Empfangszeitpunkt. Dauer = (fade_in + stay + fade_out) Ticks.
    pub(crate) title_text: Option<Vec<protocol::nbt::TextSpan>>,
    pub(crate) subtitle_text: Option<Vec<protocol::nbt::TextSpan>>,
    pub(crate) title_set_at: Instant,
    pub(crate) title_fade_in: i32,
    pub(crate) title_stay: i32,
    pub(crate) title_fade_out: i32,
    // Scoreboard: Objective-Name -> Titel der Seitenleiste; welches Objective in Slot 1
    // (Seitenleiste) haengt; je Objective die Zeilen (Holder -> (Score, optionaler Anzeigetext)).
    pub(crate) sb_objective_titles: std::collections::HashMap<String, String>,
    pub(crate) sb_sidebar_objective: Option<String>,
    pub(crate) sb_scores: std::collections::HashMap<String, std::collections::HashMap<String, (i32, Option<String>)>>,
    // Teams: Name -> (Prefix, Suffix) und Mitglied -> Teamname. Servern dienen Teams als
    // Textquelle der Seitenleiste: der Holder ist nur ein Platzhalter ("0".."15"), sichtbar
    // wird Prefix + Holder + Suffix (Vanilla: Team.decorateName).
    pub(crate) sb_teams: std::collections::HashMap<String, TeamInfo>,
    pub(crate) sb_entry_team: std::collections::HashMap<String, String>,
    // Tab-Liste: Eintraege je UUID, Kopf-/Fusszeile (leer = keine), welches Objective in
    // Slot 0 (Spielerliste) haengt, und die Darstellung der Objectives/Scores (Herzen statt
    // Zahl, Zahlenformate). Alles gehoert zur Verbindung, s. den Abbau beim Trennen.
    pub(crate) tab_entries: std::collections::HashMap<[u8; 16], TabEntry>,
    pub(crate) tab_header: Vec<protocol::nbt::TextSpan>,
    pub(crate) tab_footer: Vec<protocol::nbt::TextSpan>,
    pub(crate) sb_list_objective: Option<String>,
    pub(crate) sb_objective_hearts: std::collections::HashSet<String>,
    pub(crate) sb_objective_formats: std::collections::HashMap<String, protocol::packets::NumberFormat>,
    pub(crate) sb_score_formats: std::collections::HashMap<(String, String), protocol::packets::NumberFormat>,
    // Wird gerade ein Zahlen-Wertfeld (s. EditableValue) per Freitext bearbeitet? (Feld,
    // Live-Texteingabe). Klick auf die Zahl selbst startet das (s. window_event), Enter
    // uebernimmt, Escape oder ein Klick anderswo verwirft/uebernimmt (s. commit_edit_value).
    pub(crate) editing_value: Option<(EditableValue, String)>,
    // Dasselbe fuer einen Regler ueber Pfeil-Aktionen (s. StepSlider), erkannt an seiner
    // prev-Aktion. Uebernommen wird die Zahl so, wie sie im Feld steht (s. commit_step_edit).
    pub(crate) editing_step: Option<(ButtonAction, String)>,
    // Im Skin-Tab gewaehlter (aber noch nicht geladener) Pack-Stapel, oberstes zuerst --
    // leer = "Kein Pack". Geladen wird er, sobald man den Tab verlaesst (s.
    // apply_skin_tab). Getrennt von settings.active_skins (das IST der aktive Stapel).
    pub(crate) selected_skins: Vec<String>,
    // Was in den geladenen Packs ausgeschaltet ist -- weicht es von settings.skin_exclusions
    // ab, wird beim Verlassen des Skin-Tabs neu geladen.
    pub(crate) loaded_skin_exclusions: std::collections::BTreeMap<String, Vec<String>>,
    // Packs im Skin-Ordner mit Beschreibung und Bild (s. skin_tab::Catalog).
    pub(crate) skin_catalog: skin_tab::Catalog,
    // Gezogene Pack-Kachel im Skin-Tab.
    pub(crate) skin_drag: Option<SkinDrag>,
    // Fenster mit dem Dateibaum eines Packs (Zahnrad auf dem Pack-Bild).
    pub(crate) skin_files: Option<SkinFilesWindow>,
    // War der Skin-Tab im letzten Bild offen? Beim Verlassen wird geladen.
    pub(crate) skin_tab_was_open: bool,
    // Zuletzt ans Fenster gegebene Sichtbarkeit des System-Cursors (s. refresh_cursor_visibility).
    pub(crate) system_cursor_visible: std::cell::Cell<Option<bool>>,
    pub(crate) modifiers: ModifiersState,
    pub(crate) tokio_rt: tokio::runtime::Runtime,
    pub(crate) mode: AppMode,
    pub(crate) camera: render::Camera,
    pub(crate) move_forward: bool,
    pub(crate) move_backward: bool,
    pub(crate) move_left: bool,
    pub(crate) move_right: bool,
    pub(crate) jump_held: bool,
    pub(crate) sneak_held: bool,
    // Rechtsklick gehalten (Vanillas useKey.isPressed()) -- Quelle fuers Dauerfeuer.
    pub(crate) use_held: bool,
    // Rechtsklick-WARTESCHLANGE: jeder einzelne Druck landet hier und wird mit EINEM Klick
    // pro Tick abgearbeitet. Vanilla feuert stattdessen alle Druecke eines Ticks im selben
    // Tick ab (MinecraftClient.handleInputEvents: `while (useKey.wasPressed())`) -- bei zwei
    // Klicks innerhalb von 50 ms gehen dort also zwei Platzierungen an dieselbe Stelle raus,
    // von denen die zweite verpufft. Verteilt auf zwei Ticks zaehlt jeder Klick.
    pub(crate) use_queue: u32,
    // Hat die letzte Benutzung einen Block gesetzt? (s. tick_item_use: dann warten die
    // restlichen Klicks des Ticks auf den naechsten.)
    pub(crate) use_placed_block: bool,
    // Hat die letzte Benutzung einen geladenen Anker gezuendet? Dann die Position (s.
    // tick_item_use und anchor_wait).
    pub(crate) use_detonated_anchor: Option<(i32, i32, i32)>,
    // Letzter Versuch, Bogen/Armbrust ohne Munition zu spannen (Pfeil-Modul, s.
    // Settings::arrow_empty_feedback).
    pub(crate) arrow_empty_at: Option<Instant>,
    // Sound-Debug: angezeigter Pegel links/rechts und wann er zuletzt abgelesen wurde --
    // Cell, weil der HUD-Bau nur &self hat. Steigt sofort, faellt langsam (Spitzenanzeige).
    pub(crate) sound_meter: std::cell::Cell<(f32, f32, Option<Instant>)>,
    // Klicks halten, bis der gezuendete Anker weg ist: Position und bisher gewartete Ticks
    // (s. tick_item_use).
    pub(crate) anchor_wait: Option<((i32, i32, i32), u32)>,
    // Klicks halten, bis der Server das Ausleeren eines Wasser-/Pulverschnee-Eimers bestaetigt
    // hat: Sequenz des Ausleerens, Hotbar-Slot und bisher gewartete Ticks (s. tick_item_use).
    pub(crate) bucket_ack_wait: Option<(i32, usize, u32)>,
    // Hat die letzte Benutzung einen solchen Eimer ausgeleert? Dann ihre Sequenz.
    pub(crate) use_emptied_bucket: Option<i32>,
    // Name der aktuellen Dimensionsart (z.B. "minecraft:the_nether"), fuer den Anker.
    pub(crate) dimension_type_name: String,
    // Wartende Hotbar-Tasten (s. SlotQueue).
    pub(crate) slot_queue: SlotQueue,
    // MinecraftClient.itemUseCooldown: nur fuer das Dauerfeuer bei GEHALTENER Taste. Auf
    // einzelne Klicks wirkt er auch in Vanilla nicht (doItemUse prueft ihn nicht, es SETZT
    // ihn nur) -- deshalb hat die Warteschlange oben Vorrang vor ihm.
    pub(crate) use_cooldown: u32,
    // Ob seit dem Druck ueberhaupt eine Benutzung rausging. Nur dann ist das Loslassen etwas
    // wert -- s. tick_item_use.
    pub(crate) use_started: bool,
    // Laeuft gerade eine DURCHGEHENDE Item-Benutzung (Essen, Bogen, Schild)? Vanillas
    // ClientPlayerEntity.isUsingItem(). Steuert zweierlei: die 0.2-Daempfung der
    // Bewegungseingabe und das Ausbleiben der Wiederhol-Benutzung bei gehaltener Taste.
    pub(crate) using_item: bool,
    // Mit WELCHER Hand -- true = Nebenhand. Vanillas getActiveHand(); bei fremden Spielern
    // steht dasselbe im LIVING_FLAGS-Byte. Ohne das wuesste man zwar, DASS wir benutzen,
    // aber nicht, welcher Arm das Schild hochreisst -- und Schilde liegen in der Nebenhand.
    pub(crate) using_offhand: bool,
    // Welches Item die laufende Benutzung begonnen hat (Vanillas useItem, nur die Item-Art) --
    // fuer den lokalen Abbruch bei Item-Wechsel unter Aggressive Prediction (s.
    // App::tick_aggressive_use_cancel).
    pub(crate) using_item_id: Option<u32>,
    // Item-Abklingzeiten (Schild nach Axthieb, Enderperle, ...) -- s. item_cooldowns.
    pub(crate) item_cooldowns: item_cooldowns::ItemCooldowns,
    // Aufnahme-Animation der Hotbar (Vanillas popTime): wann Platz i zuletzt ein Item bekam
    // oder mehr davon (s. hotbar_pop_amount).
    pub(crate) hotbar_pop: [Option<Instant>; 9],
    // Abbauen -- die Felder von Vanillas ClientPlayerInteractionManager (s. attack_block).
    // breaking_pos/breaking_selected bleiben nach dem Ende stehen: isCurrentlyBreaking
    // vergleicht mit ihnen, ohne auf breaking_block zu schauen.
    pub(crate) breaking_block: bool,
    // In diesem Tick ging schon ein Abbau-Paket raus (Start, Ende oder Abbruch). Bis zum
    // ClientTickEnd darf kein Setzen/Benutzen folgen: GrimAC PacketOrderI (s. do_item_use).
    pub(crate) dig_packet_this_tick: bool,
    pub(crate) breaking_pos: Option<(i32, i32, i32)>,
    pub(crate) breaking_selected: Option<protocol::item::ItemStack>,
    pub(crate) breaking_progress: f32,
    pub(crate) block_breaking_cooldown: i32,
    // blockBreakingSoundCooldown: zaehlt die Abbau-Ticks, jeder vierte klingt (s.
    // update_block_breaking_progress).
    pub(crate) block_breaking_sound_cooldown: u32,
    // Hat ein Linksklick in diesem Tick einen Block sofort abgebaut? (bl3 in handleInputEvents)
    pub(crate) attack_broke_block: bool,
    // Eigener Spielmodus (GameMode-Index: 0 Survival, 1 Creative, 2 Adventure, 3 Spectator).
    pub(crate) game_mode: u8,
    // Gui.toolHighlightTimer: wie lange der Name des gewaehlten Items noch ueber der Hotbar
    // steht (Ticks), und woran der letzte erkannt wird (Item und Name), s. tick_held_name.
    pub(crate) held_name_timer: i32,
    pub(crate) held_name_last: Option<String>,
    // Ruestungs-HUD: je Slot (Helm, Brust, Hose, Schuhe) das zuletzt getragene Teil -- liegt es
    // nach dem Ausziehen im Inventar, steht es halb durchsichtig an seinem Platz.
    pub(crate) armor_last_worn: [Option<protocol::item::ItemStack>; 4],
    // Zuschauer sehen durch die Augen dieses Entity (SetCamera-Paket); None = die eigenen.
    pub(crate) camera_entity: Option<i32>,
    // Laufender Klick-Teleport des Zuschauers: Ziel der Fuesse (s. spectator.rs).
    pub(crate) spectator_flight: Option<DVec3>,
    // Wann je Entity zuletzt ein Speer-Treffer klang (s. kinetic_hit_sound).
    pub(crate) kinetic_hit_at: std::collections::HashMap<i32, Instant>,
    // Offenes Spielmodus-Rad und der Modus vor dem letzten Wechsel (s. gamemode_wheel.rs).
    pub(crate) gamemode_wheel: Option<gamemode_wheel::Wheel>,
    // Reconnect-Taste: seit wann gehalten (s. tick_reconnect_hold). Nach dem Ausloesen erst
    // wieder scharf, wenn die Taste los ist -- sonst verbaende ein Halten immer wieder neu.
    pub(crate) reconnect_hold: Option<Instant>,
    pub(crate) reconnect_needs_release: bool,
    pub(crate) previous_game_mode: Option<u8>,
    // Eigene Operator-Stufe 0..4 (Entity-Events 24..28 fuer den eigenen Spieler).
    pub(crate) op_level: u8,
    // Das Kreativinventar (s. creative.rs/creative_screen.rs) und die Namen der Server-
    // Registries, auf die seine Stapel verweisen.
    pub(crate) creative: creative::Creative,
    pub(crate) creative_registries: creative::Registries,
    // Die Abbau-Attribute des eigenen Spielers (s. world::mining::BreakSpeedInput).
    pub(crate) attr_block_break_speed: f32,
    pub(crate) attr_mining_efficiency: f32,
    pub(crate) attr_submerged_mining_speed: f32,
    // Die drei BEWEGUNGS-Attribute des eigenen Spielers (s. net::ClientEvent::
    // MovementAttributes). Sie kommen vom Server und ueberschreiben die Vanilla-Grundwerte:
    //   attr_movement_speed            0.1  Grundtempo -- OHNE Sprint/Schnelligkeit/Langsamkeit,
    //                                       die rechnet die Physik selbst (s. read_attributes).
    //   attr_sneaking_speed            0.3  Schleichtempo -- Schleichender Schritt hebt es an.
    //   attr_water_movement_efficiency 0.0  Wassertempo -- Wasserlaeufer hebt es an.
    // Ohne sie bewegt sich der Client mit jeder dieser Verzauberungen anders als der Server
    // rechnet -- GrimAC liest genau diese drei Attribute (MovementCheckRunner).
    pub(crate) attr_movement_speed: f64,
    // Angriffstempo vom Server ohne Item-Modifikator (s. attack_speed_attribute). None = noch
    // nichts gekommen, dann gilt Vanillas Grundwert 4.0.
    pub(crate) attr_attack_speed: Option<(f64, f64, f64)>,
    pub(crate) attr_sneaking_speed: f64,
    pub(crate) attr_water_movement_efficiency: f64,
    // Koerper-Attribute des eigenen Spielers (Schwerkraft, Sprungkraft, Seelenlaeufigkeit,
    // Groesse, Stufenhoehe; s. physics::BodyAttributes). Hier statt nur in der Physik, weil
    // die beim Teleport neu angelegt wird.
    pub(crate) attr_body: physics::BodyAttributes,
    // Reichweiten (block_interaction_range 4.5, entity_interaction_range 3.0) -- im
    // Kreativmodus legt der Server +0.5 bzw. +2.0 an und schickt sie mit.
    pub(crate) attr_block_range: f64,
    pub(crate) attr_entity_range: f64,
    // Fortlaufende Nummer fuer Block-Aktionen. Der Server spiegelt sie im Block-Changed-Ack
    // zurueck; sie MUSS streng steigen, sonst verwirft er die Aktion.
    pub(crate) block_sequence: i32,
    // Offene Platzierungs-Vorhersagen (s. predict_block_place), aeltester zuerst.
    pub(crate) pending_blocks: Vec<PendingBlock>,
    // BESTAETIGTE Platzierungen, deren Geisterblock noch stehen bleibt, bis der neu gebaute
    // Spalten-Mesh auf der GPU ist (s. resolve_pending_blocks).
    pub(crate) ghosts_awaiting_mesh: Vec<GhostAwaitingMesh>,
    // Bestaetigte eigene Platzierungen (Position, Zustand vorher, seit wann) -- Quellen fuers
    // Einblenden der Eckabdunklung im naechsten Mesh ihrer Spalte(n), s. AoFade.
    pub(crate) ao_fade_sources: Vec<((i32, i32, i32), u32, Instant)>,
    // Shader-Pfad: Blocklicht je Spalte aus dem letzten Mesh (nur Spalten, in denen etwas
    // leuchtet) -- daraus bekommen Entities, Hand und Blockentities ihr Licht (s. light_at).
    pub(crate) column_light: std::collections::HashMap<(i32, i32), world::light::ColumnBlockLight>,
    // Entity.moveDist/nextStep des eigenen Spielers: die fuer Schrittgeraeusche aufsummierte
    // Strecke und ab wann der naechste Schritt klingt (s. tick_step_sounds).
    pub(crate) step_move_dist: f32,
    pub(crate) step_next: f32,
    pub(crate) ao_fades: Vec<AoFade>,
    // Nahrung (Set Health) -- eine Sprint-Bedingung in Vanilla, s. physics::Input. Start ist
    // "satt": bis das erste Set Health kommt, darf der Sprint nicht faelschlich gesperrt sein.
    pub(crate) food_level: i32,
    // Saettigung aus Set Health (0..20), nur fuer das Saettigungs-Modul.
    pub(crate) food_saturation: f32,
    // Was der Saettigungsbalken gerade zeigt, und wann zuletzt gezeichnet: er laeuft dem Wert
    // weich nach (s. build_saturation_bar).
    pub(crate) saturation_shown: std::cell::Cell<Option<(f32, Instant)>>,
    // Wo die Ruestungszeile gerade steht (GUI-Punkte), und wann zuletzt gezeichnet: sie gleitet
    // zu ihrem Platz, wenn Absorption Herzzeilen dazu bringt (s. build_status_bars).
    pub(crate) armor_row_shown: std::cell::Cell<Option<(f32, Instant)>>,
    // Seit wann jedes Absorptionsherz da ist (Index = Herz): neue blenden von unten ein.
    pub(crate) absorption_since: std::cell::RefCell<Vec<Instant>>,
    // Die eigenen Statuseffekte: Registry-ID -> Staerke. Gefuellt aus Update Mob Effect,
    // geleert aus Remove Mob Effect.
    //
    // KEIN eigenes Herunterzaehlen der Dauer, obwohl sie im Paket steht -- und das ist eine
    // bewusste Entscheidung nach dem Jar, keine Nachlaessigkeit: LivingEntity.tickStatusEffects
    // hat zwei Zweige, und im CLIENT-Zweig ruft es nur `tickClient()` auf jede Instanz. Das
    // zaehlt zwar die Dauer herunter, entfernt aber NICHTS und nimmt keinen Attribut-
    // Modifikator zurueck; entfernt wird ausschliesslich serverseitig, worauf der Server ein
    // Remove-Paket schickt.
    //
    // Wer stattdessen selbst abliefe, waere systematisch zu FRUEH dran: die Dauer im Paket ist
    // beim Eintreffen schon um die Leitungszeit veraltet. Man liefe also die letzten Ticks
    // eines Trankes langsamer als der Server rechnet.
    //
    // Gebraucht werden bislang drei Effekte (Blindheit sperrt den Sprint, Schnelligkeit und
    // Langsamkeit aendern das Tempo), gehalten werden ALLE: das Filtern beim Empfang war
    // schon einmal der Ort, an dem eine falsche ID unbemerkt blieb.
    pub(crate) own_effects: std::collections::HashMap<i32, i32>,
    // Dieselben Effekte fuer die ANZEIGE (HUD oben rechts, Liste neben dem Inventar): mit
    // Dauer, Flags und Ankunftszeit. Die Restdauer zaehlt die Anzeige selbst herunter wie
    // Vanillas tickClient -- entfernt wird trotzdem nur auf das Remove-Paket hin.
    pub(crate) shown_effects: std::collections::HashMap<i32, ShownEffect>,
    // Was die Statusleisten ueber der Hotbar zeigen (s. build_status_bars). Alles kommt vom
    // Server: Leben/Nahrung aus Set Health, Ruestung und Maximalgesundheit aus Update
    // Attributes, Erfahrung aus Set Experience, Restluft aus dem Metadaten-Strom des eigenen
    // Entities. Die Startwerte sind Vanillas Vorgaben, damit vor dem ersten Paket keine
    // leeren Leisten dastehen.
    // Spielername je UUID (aus der Spielerliste) -- Grundlage der Namensschilder.
    pub(crate) player_names: std::collections::HashMap<[u8; 16], String>,
    pub(crate) health: f32,
    pub(crate) max_health: f32,
    pub(crate) armor: i32,
    // PlayerEntity.getAbsorptionAmount -- aus den eigenen Metadaten (s. ClientEvent::EntityMeta).
    pub(crate) absorption: f32,
    pub(crate) xp_level: i32,
    pub(crate) xp_progress: f32,
    // Restluft in Ticks; 300 = voll (Entity.getMaxAir).
    pub(crate) air_ticks: i32,
    // Vanillas PlayerEntity.ticksSinceLastAttack -- Grundlage der Angriffs-Aufladung
    // (s. apply_attack_slowdown). Startwert hoch genug, dass der allererste Schlag als voll
    // aufgeladen zaehlt, genau wie beim frisch gespawnten Vanilla-Spieler.
    pub(crate) ticks_since_last_attack: i32,
    // Womit zuletzt getickt wurde -- ein Wechsel setzt die Aufladung zurueck (s.o.).
    pub(crate) last_attack_hand_item: Option<&'static str>,
    // Im letzten Tick hat das Item in der Haupthand gewechselt: die Aufladung faellt zu
    // Beginn des naechsten auf 0 (s. Tickschleife).
    pub(crate) attack_hand_changed: bool,
    pub(crate) sprint_held: bool,
    // Physischer Tastenzustand je Aktion (unabhaengig von move_forward/.../sprint_held, den
    // EFFEKTIVEN Werten) -- noetig, um im Umschalten-Modus die steigende Flanke (Tastendruck,
    // nicht Loslassen) zu erkennen, s. apply_key_action.
    pub(crate) keybind_physical_down: [bool; KeyBindAction::ALL.len()],
    // Was GERADE physisch gedrueckt ist -- Tasten und Maustasten, unabhaengig davon, ob der
    // Druck im aktuellen Zustand etwas ausgeloest hat (Menue, Chat, Ladebildschirm). Daraus
    // stellt restore_held_controls die gehaltenen Aktionen wieder her, s. dort.
    pub(crate) binds_down: Vec<Bind>,
    // Je Taste der letzte Druck und ob er ein Doppelklick war (zweiter Druck innerhalb von
    // INVENTORY_DOUBLE_CLICK) -- die Keystrokes zeigen ihn in Akzent 2.
    pub(crate) bind_presses: Vec<(Bind, Instant, bool)>,
    // Wann eine Aktion zuletzt GEDRUECKT wurde, als laufende Nummer (0 = noch nie). Braucht
    // nur SnapTap (s. Settings::snap_tap_enabled): bei zwei gegensaetzlichen Richtungen
    // gewinnt die groessere Nummer, also die zuletzt gedrueckte Taste.
    pub(crate) keybind_press_order: [u64; KeyBindAction::ALL.len()],
    pub(crate) keybind_press_counter: u64,
    // Scroll-Offset (in Zeilen) der Tastenbelegungs-Tabelle im Steuerung-Screen, s.
    // build_settings_controls_tab.
    pub(crate) keybind_scroll: smooth_scroll::SmoothScroll,
    // Erste sichtbare Zeile der Haupt-Einstellungen (s. settings_row_y/sync_settings_scroll).
    pub(crate) settings_scroll: smooth_scroll::SmoothScroll,
    // Zuletzt an den Server gesendete Player-Input-Flags (protocol::packets::
    // player_input_payload) -- nur bei Aenderung neu senden.
    pub(crate) last_sent_player_input: Option<(bool, bool, bool, bool, bool, bool, bool)>,
    // Eingabe-Aktionen, die noch auf den naechsten Tick warten (s. PendingInputAction und
    // flush_input_actions): Vanilla verschickt sie NICHT im Moment des Tastendrucks, sondern
    // gesammelt im Tick davor -- was Grims Post-Check erzwingt.
    pub(crate) pending_input_actions: Vec<PendingInputAction>,
    // Zuletzt an den Server gesendeter Hotbar-Slot (ClientPlayerInteractionManager.
    // lastSelectedSlot). Nur bei Abweichung ein Paket, und nur zum Tickbeginn. Startwert 0
    // wie Vanillas uninitialisiertes int-Feld -- sonst ginge beim Join ein
    // "Slot 0"-Paket raus, das ein echter Client nie schickt.
    pub(crate) last_synced_slot: usize,
    // Aus Login (play) (0x30, s. net::ClientEvent::OwnEntityId) -- fuer serverbound Pakete,
    // die die eigene EntityId brauchen (Player Command: Sprint-Start/Stop).
    pub(crate) own_entity_id: Option<i32>,
    // Bossbars in der Reihenfolge, in der der Server sie angelegt hat (BossBarHud.bossBars ist
    // eine LinkedHashMap).
    pub(crate) boss_bars: Vec<BossBarState>,
    // Worauf der eigene Spieler sitzt (s. vehicle::OwnRide). None = zu Fuss.
    pub(crate) own_ride: Option<vehicle::OwnRide>,
    // Drehung, die das gelenkte Boot im letzten Tick an den Spieler weitergegeben hat, und wie
    // viel davon die Kamera schon mitgemacht hat (0..1). Vanilla zeichnet die Blickrichtung
    // eines Mitfahrers zwischen zwei Ticks interpoliert (Entity.getYaw(tickProgress)); hier
    // wird die Drehung dafuer ueber die Bilder bis zum naechsten Tick verteilt.
    pub(crate) ride_yaw_spin: (f32, f32),
    // Eigene Spieler-UUID aus Login-Success (s. net::ClientEvent::OwnUuid) -- der Schluessel,
    // unter dem der eigene Skin in `skins` liegt. Ohne sie zeigt das eigene Modell in der
    // 3.-Person-Ansicht dauerhaft den Standard-Skin, obwohl der echte schon geladen ist.
    pub(crate) own_uuid: Option<[u8; 16]>,
    // Wohin das Zahnrad (bzw. Esc auf einer Kategorie) die Einstellungen verlaesst.
    pub(crate) settings_return: MenuScreen,
    // Woher der HUD-Editor geoeffnet wurde (Pausemenue oder Einstellungen) -- dorthin geht
    // es zurueck.
    pub(crate) hud_editor_return: MenuScreen,
    // Benachrichtigungen fuer die Glocke oben rechts: die letzten NOTIFICATION_HISTORY (Titel,
    // Text, Art, wann), wie viele davon schon gesehen sind, und ob die Liste offen ist.
    pub(crate) notification_history: std::collections::VecDeque<(String, String, notifications::Kind, Instant)>,
    // Verbindungs-Warnungen (s. check_packet_gap, note_lagback): wann das letzte Ereignis vom
    // Server kam, die stehende "Keine Pakete"-Karte, die Lag-backs der letzten Sekunden und
    // die Rubberbanding-Karte, die mitzaehlt.
    pub(crate) last_server_event: Instant,
    // Ping-IDs des Servers (Anticheat-Transaktionen), fuers Debug-Overlay.
    pub(crate) transactions: transactions::Transactions,
    pub(crate) packet_gap_note: Option<notifications::NotificationId>,
    pub(crate) lagback_note: Option<(notifications::NotificationId, u32, Instant)>,
    pub(crate) rubberband_note: Option<(notifications::NotificationId, u32)>,
    pub(crate) notifications_seen: usize,
    pub(crate) notification_panel_open: bool,
    // Wann die Glocke zuletzt auf- oder zugemacht wurde: das Fenster faehrt von da an
    // NOTIFICATION_PANEL_SLIDE lang herein bzw. hinaus.
    pub(crate) notification_panel_since: Instant,
    // Herausfallende geloeschte Zeichen (s. ERASE_FALL): die Eingabe-Texte des letzten
    // Frames, ob seitdem Ruecktaste/Entf/Strg+X kam, und was gerade faellt.
    pub(crate) typed_prev: Vec<TypedRun>,
    // Wann jeder UI-Klang zuletzt spielte (Index wie ui_sound::UiSound::ALL), fuer den
    // Mindestabstand.
    pub(crate) ui_sound_last: [Option<Instant>; ui_sound::UiSound::ALL.len()],
    // Mit welchem Pegel der Klang zuletzt kam (s. ui_sound::repeat_level), 1 = voll.
    pub(crate) ui_sound_level: [f32; ui_sound::UiSound::ALL.len()],
    pub(crate) erase_pending: bool,
    // Wo der Schreibcursor in diesem Frame steht (s. push_caret) und wo im letzten -- das
    // Loeschen laesst die Zeichen dort herausfallen (s. erased_pieces).
    pub(crate) caret_drawn: std::cell::Cell<Option<(f32, f32)>>,
    pub(crate) caret_prev: Option<(f32, f32)>,
    pub(crate) falling_glyphs: Vec<FallingGlyph>,
    // Zaehler fuer den Zufall der fallenden Zeichen (Weg, Drift, Drehung).
    pub(crate) erase_seed: u32,
    // Gleitender Schreibcursor (s. CARET_GLIDE) und gleitender Chat-Text (s. ChatSlide).
    pub(crate) caret_anim: std::cell::Cell<Option<CaretAnim>>,
    // Wohin die Markierung je Zeile gleitet (s. glide_selection).
    pub(crate) selection_glide: std::cell::RefCell<rustc_hash::FxHashMap<i32, SelectionGlide>>,
    // Klaenge der Markierung (s. selection_sounds): wie viele Zeichen im letzten Bild markiert
    // waren, welcher Klang die naechste Aenderung meldet (Doppel-/Dreifachklick) und ob seitdem
    // links geklickt oder Strg+D gedrueckt wurde.
    pub(crate) selection_sound_chars: usize,
    pub(crate) selection_sound_hint: Option<ui_sound::UiSound>,
    pub(crate) selection_sound_deselect: bool,
    // Hover-Animation der Knoepfe, nach ihrer Flaeche (s. hover_level).
    pub(crate) hover_anims: std::cell::RefCell<rustc_hash::FxHashMap<(i32, i32, i32, i32), HoverAnim>>,
    // Zuletzt mit Hover gezeichnete Flaeche und wann (s. hover_level): Hover-Klang auch fuer
    // Elemente, die nicht in buttons() stehen (Dialoge, Kontextmenues, Kontenliste ...).
    pub(crate) hover_drawn: std::cell::Cell<Option<((i32, i32, i32, i32), Instant)>>,
    pub(crate) chat_slide: std::cell::RefCell<Option<ChatSlide>>,
    pub(crate) chat_gap: std::cell::RefCell<Option<ChatGap>>,
    pub(crate) chat_recall: Option<ChatRecall>,
    // Fuer "laeuft seit" unter der Uhr.
    pub(crate) app_started: Instant,
    // Schieberegler: per Rechtsklick vergroesserte rechte Enden, und an welchem gerade
    // gezogen wird.
    pub(crate) slider_drag: Option<EditableValue>,
    // Regler mit Rechtsklick gefasst: solange die rechte Taste gehalten wird, reicht er bis zur
    // hoechsten erlaubten Grenze (s. slider_max).
    pub(crate) slider_extended: bool,
    // Gezogener Regler ueber Pfeil-Aktionen (s. StepSlider), erkannt an seiner prev-Aktion.
    pub(crate) step_drag: Option<ButtonAction>,
    // Mit der rechten Taste gefasst (Bahn bis zur harten Grenze) und das beim Fassen
    // festgehaltene rechte Ende -- es wandert waehrend des Ziehens nicht mit dem Wert.
    pub(crate) step_extended: bool,
    pub(crate) step_drag_end: f64,
    // Eigene Blickrichtung der Kamera (Gier, Neigung in Grad), solange sie vom Spieler
    // geloest ist: Freelook oder Schulterkamera mit "Kamera frei vom Koerper" (s.
    // update_camera_look). Die Maus dreht dann sie statt des Spielers.
    pub(crate) cam_look: Option<(f32, f32)>,
    // Freelook-Taste aktiv; die Ansicht davor und die Kamerarichtung davor (bei geloester
    // Schulterkamera), beides kommt beim Loslassen zurueck.
    pub(crate) freelook_held: bool,
    pub(crate) freelook_before: Option<(Perspective, Option<(f32, f32)>)>,
    // Schulterkamera: Seite (1 = wie eingestellt, -1 = gespiegelt) und die gezeigte, die ihr
    // weich folgt.
    pub(crate) shoulder_side: f32,
    pub(crate) shoulder_side_shown: (f32, Instant),
    // Kamera frei vom Koerper: bis wann der Spieler noch dorthin schaut, wo die Kamera zielt.
    pub(crate) aim_until: Option<Instant>,
    // Deckkraft des eigenen Spielers in der 3. Person (Settings::shoulder_fade), je Bild aus
    // dem Abstand der Kamera.
    pub(crate) own_player_alpha: f32,
    // Einstellungssuche: Eingabe, Treffer der letzten Suche (Enter) und wonach dabei gesucht
    // wurde (None = noch nicht gesucht), dazu die Zeile, die nach einem Klick kurz aufleuchtet.
    pub(crate) settings_search: String,
    pub(crate) search_hits: Vec<SearchHit>,
    pub(crate) search_done: Option<String>,
    pub(crate) search_flash: Option<(SearchPlace, Instant)>,
    // Angezeigte Daumen-Stellen (s. App::glide_slider): der Daumen gleitet zum Wert, statt
    // hinzuspringen.
    pub(crate) slider_glide: std::cell::RefCell<Vec<(SliderKey, f32, Instant)>>,
    pub(crate) persist_paused: bool,
    // Zuletzt gesehener Stand der Config-Dateien und wann zuletzt nachgesehen wurde -- fuer das
    // Nachladen, wenn die Datei von aussen geaendert wird (s. poll_config_file).
    pub(crate) config_seen: Option<ConfigStamp>,
    pub(crate) config_checked: Instant,
    // Offenes Farbfenster (s. ColorPopup).
    pub(crate) color_popup: Option<ColorPopup>,
    // Offene Aufklappliste (s. Choice), liegt wie das Farbfenster ueber allem.
    pub(crate) dropdown: Option<Dropdown>,
    // Linke Taste in einer Liste gedrueckt (s. App::drag_scroll_grab): bis zum Loslassen
    // wartet der Klick, ob daraus ein Ziehen wird.
    pub(crate) drag_scroll: Option<(ScrollList, smooth_scroll::DragGrip)>,
    // Der Druck schloss nur eine Aufklappliste: ohne Ziehen kein Klick beim Loslassen.
    pub(crate) drag_scroll_no_click: bool,
    // Hinterlegung des aktiven Reiters (Inhaltskoordinaten), gleitet zum neuen Abschnitt.
    pub(crate) section_slide: Option<(MenuScreen, Rect)>,
    // Gedrueckter Einstellungsknopf (s. menu/press.rs); true = sein Klick wartet dort aufs
    // Loslassen, nicht in drag_scroll.
    pub(crate) settings_press: Option<(ButtonAction, bool)>,
    // Einziehen und Zurueckfedern: Knopf, gedrueckt seit, losgelassen (seit, Groesse dabei).
    pub(crate) press_anim: Option<(ButtonAction, Instant, Option<(Instant, f32)>)>,
    // Abschnitt, der nach einem Klick auf seinen Reiter kurz gelb aufleuchtet.
    pub(crate) section_flash: Option<(MenuScreen, usize, Instant)>,
    // Beim Verbinden automatisch gewaehlte Server-Config (s. menu/config.rs): die vorher aktive
    // und die des Servers. Beim Trennen kommt die vorherige zurueck.
    pub(crate) server_config_return: Option<(String, String)>,
    // Die laufenden Addons (s. addons.rs); None bis zum ersten Durchlauf der Ereignisschleife.
    pub(crate) addons: Option<addons::Addons>,
    // Letztes Tick-Ereignis an die Addons und letzte Chatzeile eines Addons (gedrosselt).
    // Letzter Frame-Hook (fuer dt) und die zuletzt gemeldete Zahl eigener Tode (Hook::Death).
    pub(crate) addon_frame_at: Instant,
    pub(crate) addon_deaths_seen: u32,
    pub(crate) addon_chat_at: Option<Instant>,
    // Startwerte und Arten der Addon-Einstellungen, Schluessel "addon/modul/key" (fuer
    // Call::GetSetting, das waehrend eines Aufrufs nicht an die Addons selbst kommt).
    pub(crate) addon_setting_defs: std::collections::HashMap<String, addons::SettingDef>,
    // Bewegungstasten, die Addons druecken (s. Call::SetInput), je Addon-Index.
    pub(crate) addon_inputs: std::collections::HashMap<usize, coral_addon::Input>,
    // Hoert ein Addon Hook::PacketIn bzw. Hook::PacketOut? (s. rebuild_addon_registry)
    pub(crate) addon_raw_in: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) addon_hold_packets: bool,
    // Wann der Erwaehnungston (Modul Eigenname) zuletzt klang.
    pub(crate) ego_sound_at: Option<Instant>,
    // Seiten-Scroll der uebrigen Einstellungsseiten (s. MenuScreen::settings_page), je
    // settings_key. Render behaelt settings_scroll, das gespeichert wird.
    pub(crate) page_scrolls: std::collections::BTreeMap<&'static str, smooth_scroll::SmoothScroll>,
    // Zuletzt angeklickter Abschnitts-Reiter: (Seite, Abschnitt, erste Zeile danach). Bleibt
    // hervorgehoben, solange die Seite so steht -- ein Abschnitt am Ende laesst sich nicht
    // ganz nach oben scrollen und waere sonst nie der aktive.
    pub(crate) section_click: Option<(MenuScreen, usize, usize)>,
    // Ob die gespeicherten Scrollstaende schon in die Felder uebernommen sind (beim ersten
    // Oeffnen der Einstellungen, s. open_settings) -- vorher schreibt persist_settings sie nicht
    // zurueck, sonst ueberschriebe ein frueher Speichervorgang sie mit Nullen.
    pub(crate) scrolls_loaded: bool,
    // Aktive Kameraperspektive (F5). Rein clientseitig -- der Server erfaehrt davon nichts,
    // die gesendete Blickrichtung bleibt in jeder Perspektive die des Spielers.
    pub(crate) perspective: Perspective,
    // Zuletzt an den Server gesendeter Sprint-Zustand (protocol::packets::
    // player_command_sprint_payload) -- NICHT dasselbe Paket wie player_input_payload: erst
    // dieses hier ruft serverseitig tatsaechlich setSprinting() auf (s. dortigen Kommentar),
    // ohne das bleibt der Server bei der Steh-Hitbox, egal was wir lokal an Pose berechnen.
    pub(crate) last_sent_sprinting: Option<bool>,
    pub(crate) last_movement_update: Instant,
    // Skins der sichtbaren Spieler. `slot` ist der reservierte Platz im Entity-Atlas (s.
    // assets::entity_atlas), `slim` das Modell. Ein Eintrag entsteht, sobald die Spielerliste
    // eine Textur-Property nennt; das Bild kommt asynchron nach (s. request_skin).
    pub(crate) skins: std::collections::HashMap<[u8; 16], PlayerSkin>,
    // Reihenfolge der Slot-Belegung, aeltester zuerst. Sind alle Plaetze voll, wird bevorzugt
    // ein Platz neu belegt, dessen Spieler GERADE NICHT ZU SEHEN ist -- erst wenn alle
    // Platzinhaber sichtbar sind, muss der aelteste weichen (s. claim_atlas_slot).
    //
    // Reine Belegungsreihenfolge war zu wenig: der Spieler direkt vor einem konnte vom
    // naechstbesten Tabellenlisten-Eintrag verdraengt werden, obwohl der 500 Bloecke weit
    // weg stand, und der Skin sprang auf Default zurueck.
    pub(crate) skin_slot_order: std::collections::VecDeque<([u8; 16], usize)>,
    // Weitere Spieler, die auf eine GERADE laufende Skin-/Umhang-Anfrage warten (Schluessel
    // ist die URL). Sonst zeigt die Tab-Liste fremde Spielerkoepfe:
    // Server fuellen die Tab-Liste mit vielen Eintraegen, die dieselbe Skin-URL tragen, und
    // die URL-Sperre in requested_skins liess nur den ersten laden. Alle anderen blieben beim
    // Standard-Skin ihrer (zufaelligen) UUID. Dasselbe traf geteilte Umhaenge.
    pub(crate) skin_waiters: std::collections::HashMap<String, Vec<[u8; 16]>>,
    // Dasselbe fuer die Umhang-Plaetze -- getrennte Reihe, also auch eine eigene Reihenfolge.
    pub(crate) cape_slot_order: std::collections::VecDeque<([u8; 16], usize)>,
    // Gesicht + Hutlage je Spieler fuer die Tab-Liste (s. assets::HEAD_SLOTS): Platz im
    // Entity-Atlas und die 16x8 Pixel selbst, damit sie nach einem Neuaufbau des Atlas
    // (Resourcepack-Wechsel) ohne neuen Download wieder hochgeladen werden koennen.
    pub(crate) tab_heads: std::collections::HashMap<[u8; 16], (usize, Vec<u8>)>,
    // Schildtexte je Blockposition (s. protocol::sign), aus Chunk und Block Entity Data.
    pub(crate) signs: std::collections::HashMap<(i32, i32, i32), protocol::sign::SignText>,
    // Platzierte Spielerkoepfe: Position -> Skin-Schluessel (s. head_skin_key). Koepfe ohne
    // Profil fehlen hier und bekommen den Standard-Skin.
    pub(crate) placed_heads: std::collections::HashMap<(i32, i32, i32), [u8; 16]>,
    // Offener Schild-Editor (Vanillas SignEditScreen), s. open_sign_editor.
    pub(crate) sign_edit: Option<SignEdit>,
    // Offenes Buch aus der Hand (BookViewScreen), s. open_book.
    pub(crate) book_view: Option<book::BookView>,
    // Offenes Buch mit Feder (BookEditScreen/BookSignScreen), s. open_book_editor; seit wann
    // (der Cursor blinkt ab da) und ob gerade mit gedrueckter Taste markiert wird.
    pub(crate) book_edit: Option<book::BookEdit>,
    pub(crate) book_edit_since: Instant,
    pub(crate) book_edit_dragging: bool,
    // Kartenstaende (map_item_data) und ihre Bilder in den Kartenplaetzen des Entity-Atlas: je
    // Platz (Karte, im Rahmen?, Stand des Bildes, zuletzt gebraucht im Bild Nr.).
    pub(crate) maps: world::map::Maps,
    pub(crate) map_slots: Vec<Option<MapSlot>>,
    pub(crate) map_frame_counter: u64,
    // Bilder der Kartenmarkierungen (textures/map/decorations/*.png), einmal geladen.
    pub(crate) map_sprites: std::collections::HashMap<String, Option<(u32, Vec<u8>)>>,
    // Namensfeld des Ambosses (AnvilScreen.name): Text und fuer welchen Stapel in Slot 0 er
    // zuletzt gesetzt wurde (s. sync_anvil_name).
    pub(crate) anvil_name: String,
    pub(crate) anvil_name_for: Option<String>,
    // Textur-Property je Kopf-Schluessel fuer Koepfe, die andere halten oder tragen (s.
    // request_head_skins), beim Merken einmal gelesen: None = die Property nennt keinen Skin
    // (oder keinen von Mojangs Host). Gehoert zur Verbindung.
    pub(crate) head_textures: std::collections::HashMap<[u8; 16], Option<auth::SkinRef>>,
    // Kopf-Schluessel, deren Profil schon bei Mojang nachgeschlagen wird oder wurde (s.
    // note_head_profile) -- jeder hoechstens einmal je Verbindung.
    pub(crate) head_lookups: std::collections::HashSet<[u8; 16]>,
    // Die geladenen Skins der gespeicherten Konten (Pixel, slim), fuer ALLE Tabs: nach einem
    // Tab-Wechsel oder einem neuen Pack steht der Kopf oben sofort wieder im Atlas, ohne erst
    // neu geladen zu werden (s. request_account_heads).
    pub(crate) account_skins: std::collections::HashMap<[u8; 16], (Vec<u8>, bool)>,
    // Geladene Skin-Pixel je URL, ueber Verbindungen, Transfers und Tabs hinweg (die URL nennt
    // den Inhalt, er aendert sich nie). Ein Skin, der aus dem Atlas verdraengt oder nach einem
    // Transfer wieder gebraucht wird, kommt so ohne neuen Download zurueck (s. request_skin).
    pub(crate) skin_pixels: std::collections::HashMap<String, Vec<u8>>,
    // Was Mojang zu einem Kopf-Profil ohne Textur geliefert hat (s. note_head_profile), ebenso
    // ueber Verbindungen hinweg: sonst schluege jeder Transfer dieselben Koepfe neu nach.
    pub(crate) head_profile_cache: std::collections::HashMap<[u8; 16], Option<String>>,
    // Fehlgeschlagene Profil-Abfrage eines Kontokopfs: erst ab diesem Zeitpunkt erneut (s.
    // request_account_heads, das jedes Bild laeuft).
    pub(crate) account_head_retry: std::collections::HashMap<[u8; 16], Instant>,
    // Konto, auf das gerade gewechselt wird (Schluessel, seit wann): oben und in den Listen
    // schon als gewaehlt, mit Ladepunkten, bis die Anmeldung durch ist (s. use_account).
    pub(crate) account_switching: Option<(String, Instant)>,
    pub(crate) tab_head_order: std::collections::VecDeque<([u8; 16], usize)>,
    // Welche Textur-URLs GERADE UNTERWEGS sind (oder endgueltig gescheitert). Verhindert,
    // dass dieselbe Textur bei jedem Spielerlisten-Update erneut geladen wird -- Server
    // schicken die Liste oft.
    //
    // Wichtig: ein ERFOLGREICH geladener Skin wird hier wieder ausgetragen. Frueher blieb er
    // fuer immer stehen, und damit war ein aus dem Atlas verdraengter Skin unwiederbringlich
    // weg: `handle_player_info` stieg bei jeder weiteren Meldung sofort wieder aus, der
    // Spieler trug bis zum Verbindungsende den Standard-Skin.
    pub(crate) requested_skins: std::collections::HashSet<String>,
    // Woher der Skin eines Spielers kommt. Gebraucht, um ihn nach einer Verdraengung
    // NACHZULADEN, ohne auf ein neues Spielerlisten-Paket warten zu muessen -- viele Server
    // schicken die Liste nur einmal beim Betreten.
    pub(crate) skin_sources: std::collections::HashMap<[u8; 16], auth::SkinRef>,
    // Fehlgeschlagene Ladeversuche je URL. Seit ein verdraengter Skin wieder angefordert
    // werden DARF, braucht es eine Grenze: ohne sie wuerde eine Textur, die der Host gerade
    // nicht ausliefert, in jedem Frame erneut angefragt.
    pub(crate) skin_retries: std::collections::HashMap<String, u8>,
    // Schrittphase des EIGENEN Spielers (die fremden fuehren sie selbst, s.
    // world::Entity::limb_swing). Waechst pro Physik-Tick um die aktuelle Amplitude.
    // Schrittanimation des eigenen Spielers -- derselbe Animator wie bei fremden Entities
    // (s. world::LimbAnimator), gefuettert mit der tatsaechlich zurueckgelegten Strecke.
    pub(crate) own_limbs: world::LimbAnimator,
    // Eigenes Alter in Ticks fuer die Ruhe-Bewegung der Arme (s. world::biped_pose).
    pub(crate) own_age_ticks: f32,
    // Rest-Ticks des eigenen Schadens-Aufblitzens (nur in der 3. Person sichtbar).
    //
    // Der Server schickt fuer den EIGENEN Spieler kein Damage-Paket -- ServerWorld verteilt
    // es nur an die anderen Beobachter. Vanilla erkennt den eigenen Treffer stattdessen am
    // Sinken der Gesundheit (ClientPlayerEntity.updateHealth: `if (getHealth() - health > 0)
    // { maxHurtTime = 10; hurtTime = maxHurtTime; }`), und genau so wird es hier gemacht.
    pub(crate) own_hurt_ticks: u8,
    // Ticks seit dem letzten eigenen Treffer (u32::MAX = nie). Treibt die Schadens-Faerbung,
    // deren Dauer einstellbar ist (Settings::damage_tint_ticks) -- own_hurt_ticks selbst
    // bleibt bei Vanillas 10.
    pub(crate) own_ticks_since_hurt: u32,
    // Dieselben zwei Merker wie bei fremden Entities (s. world::Entity::on_crit), nur fuer den
    // eigenen Spieler: stammt der laufende Blitz von einem Crit, und wartet eine Crit-Meldung
    // gerade auf ihren Treffer?
    //
    // Getrennt gefuehrt, weil der eigene Treffer aus einer ANDEREN Quelle kommt: nicht aus dem
    // Schadenspaket (das bekommt man ueber sich selbst nie), sondern aus der sinkenden
    // Gesundheit. Das Crit-Paket dagegen bekommt man sehr wohl -- der Server schickt es mit
    // sendToNearbyPlayers, also einschliesslich des Getroffenen.
    // Tageszeit in Ticks und ob sie laeuft (Set Time, s. packets::read_world_time). Der Server
    // schickt das nur alle 20 Ticks -- dazwischen zaehlt der Client selbst hoch, sonst ruckte
    // der Himmel einmal pro Sekunde.
    pub(crate) world_time_of_day: i64,
    pub(crate) world_time_ticking: bool,
    // Seit dem Verbinden noch keine Uhrzeit vom Server: der Pack-Himmel laedt dann vor, ist
    // aber noch nicht zu sehen (sonst stuende er erst zur alten Zeit da und sprang dann).
    pub(crate) sky_time_pending: bool,
    // Nach einem Dimensionswechsel (seit dann): der Pack-Himmel der neuen Welt laedt vor und
    // erscheint erst, wenn er ganz da ist -- sonst kaemen die Ebenen einzeln nacheinander.
    pub(crate) sky_world_pending: Option<Instant>,
    pub(crate) own_hurt_by_crit: bool,
    // Entity.FLAGS Bit 0 des eigenen Spielers: brennt (Feuer um die Figur in der 3. Person).
    pub(crate) own_on_fire: bool,
    // Mehr Animationen: Handtausch der eigenen Figur und Glaettung aller Spielerfiguren
    // (s. app/anims.rs).
    pub(crate) own_swap: world::more_anims::SwapWatch,
    pub(crate) anim_smoothing: std::cell::RefCell<std::collections::HashMap<i32, (Instant, world::more_anims::Smoothing)>>,
    pub(crate) anim_petting: std::collections::HashSet<i32>,
    pub(crate) own_last_sword: Option<world::HeldItem>,
    // Item ansehen (s. KeyBindAction::InspectHeld): Item-ID der Haupthand beim Start (0 = leer)
    // und die seither vergangenen Ticks.
    pub(crate) own_inspect: Option<(u32, u32)>,
    // Welche Animation beim Start lief; aendert sie sich, endet das Ansehen.
    pub(crate) own_inspect_kind: Option<world::item_inspect::Kind>,
    // Zuletzt ins Log geschriebener Griff der Ego-Hand (Item, gewaehltes Modell), s. emit_holding_arm.
    pub(crate) hand_grip_logged: std::cell::RefCell<Option<(u32, Option<String>)>>,
    // Hand-Debug: die Pfeiltasten verschieben das Item statt der Faust (Knopf rechts oben).
    pub(crate) hand_debug_item_mode: bool,
    // Hand-Debug: die Tasten stellen beim gespannten Bogen die Hand an der Sehne ein (Knopf
    // "Sehnenhand", s. App::string_hand).
    pub(crate) hand_debug_string: bool,
    // Hand-Debug: statt des gehaltenen Items ein Bogen in der Haupthand, 0 = ungespannt, 1..=3 =
    // Spannstufe pulling_0..2 (Knoepfe "Bogen", "Bogen 0" ... , s. hand_debug_buttons), mit seinem
    // Stapel. Nur solange Hand-Debug laeuft (s. hand_debug_bow_active).
    pub(crate) hand_debug_bow: Option<(u8, protocol::item::ItemStack)>,
    pub(crate) lantern_swing: std::cell::RefCell<std::collections::HashMap<(i32, bool), world::more_anims::LanternSwing>>,
    pub(crate) anim_hugging: std::collections::HashSet<i32>,
    // Entity.FROZEN_TICKS des eigenen Spielers (ab 140 Frost-Herzen).
    pub(crate) own_frozen_ticks: i32,
    // Hardcore-Welt laut Login (Hardcore-Herzen).
    pub(crate) hardcore: bool,
    // Richtung, in die die Hurt-Cam beim naechsten Treffer rollt -- wechselt mit jedem.
    pub(crate) hurt_cam_side: f32,
    // Stapel-Wurf (s. inventory_throw_sweep): der zuletzt betretene Slot, solange der Zeiger
    // noch darauf steht.
    pub(crate) throw_sweep_slot: Option<usize>,
    // Eigene Trank-Wirbel (LivingEntity.POTION_SWIRLS/_AMBIENT) und Unsichtbarkeit -- wir
    // stehen nicht in `entities`, s. tick_potion_swirls.
    pub(crate) own_potion_swirls: Vec<u32>,
    pub(crate) own_potion_swirls_ambient: bool,
    pub(crate) own_invisible: bool,
    pub(crate) own_glowing: bool,
    pub(crate) own_crit_grace: u8,
    // Ob schon ein Set Health angekommen ist (Vanillas healthInitialized). Ohne diese Sperre
    // waere der ERSTE Gesundheitswert nach dem Betreten ein "Sturz" von 20 auf den echten
    // Wert -- man blitzte beim Joinen auf, ohne getroffen worden zu sein.
    pub(crate) own_health_initialized: bool,
    // Koerper-/Kopfdrehung des eigenen Spielers (s. world::BodyYaw). Der Koerper laeuft dem
    // Blick nach, der Kopf folgt ihm sofort -- das ist die Traegheit, die man in Vanilla sieht.
    // Ticks, die das eigene Item schon ununterbrochen benutzt wird (fuer isBlocking).
    pub(crate) own_use_ticks: u32,
    // Das laufende bzw. zuletzt beendete Essen hat der Consumable-Optimizer durchgedrueckt
    // (s. server_still_eating). Neu bei jeder neuen Benutzung.
    pub(crate) eat_held_through: bool,
    // Ticks, die neue Rechtsklicks nach dem lokalen Fertigessen noch warten (s. tick_item_use).
    pub(crate) eat_next_use_delay: u32,
    pub(crate) own_body_yaw: world::BodyYaw,
    // Geglaettete Blickrichtung fuer das Nachschwingen der Ego-Hand. Vanilla
    // (ClientPlayerEntity.tickMovement) fuehrt renderYaw/renderPitch mit, die der echten
    // Blickrichtung mit 0.5 pro Tick nachlaufen; die DIFFERENZ zwischen beiden dreht die Hand
    // um ein Zehntel davon. Dadurch bleibt die Hand beim schnellen Umsehen kurz zurueck.
    pub(crate) own_render_yaw: f32,
    pub(crate) own_render_pitch: f32,
    pub(crate) own_last_render_yaw: f32,
    pub(crate) own_last_render_pitch: f32,
    // Hoehe der Hand: 1 = ganz nach unten weggetaucht, 0 = oben. Folgt der Angriffs-Aufladung
    // (equip_target/equip_step); ein Slotwechsel setzt sie ganz nach unten.
    pub(crate) own_equip_progress: f32,
    // Stand vor dem letzten Tick, fuers Interpolieren zwischen zwei Ticks.
    pub(crate) own_last_equip_progress: f32,
    // Welches klickbare Element zuletzt unter dem Zeiger lag. Der Hover-Klang haengt am
    // WECHSEL, nicht an der Mausbewegung -- so wie osu!lazer ihn an OnHover haengt und nicht
    // an jedes Bewegungsereignis. Ohne diesen Vergleich waere es ein Dauerfeuer.
    pub(crate) hovered_button: Option<HoverTarget>,
    // Die mitgelieferten Klangaufnahmen (Kenney, CC0 -- s. audio::bank). Entpackt beim ersten
    // Abspielen und danach gehalten.
    pub(crate) sounds: audio::bank::SoundBank,
    // Fortlaufend, damit aufeinanderfolgende Tipptoene mit Zufalls-Tonhoehe (s.
    // TYPE_SOUND_PITCH_RANGE) verschieden ausfallen -- derselbe Seed zweimal haette denselben
    // "Zufallswert" ergeben.
    pub(crate) type_pitch_seed: u32,
    // Die beiden Arm-Modelle, einmal gebaut (breit/schmal, s. world::player_arm_model).
    pub(crate) hand_model_wide: world::Model,
    pub(crate) hand_model_slim: world::Model,
    // Restliche Ticks der Schlag-Animation. Vanilla richtet den Koerper waehrend eines
    // Schlags zur Blickrichtung aus (LivingEntity.tick, `handSwingProgress > 0`), sonst
    // schluege man sichtbar an der Blickrichtung vorbei. Die 6 Ticks sind Vanillas
    // Schwungdauer fuer Spieler.
    pub(crate) own_swing_ticks: u32,
    // Nachhinkende Position + Laufgroessen fuer den eigenen Umhang (s. world::CapeState).
    pub(crate) own_cape: world::CapeState,
    // Modul-Menue offen? Ein In-Welt-Overlay wie das Inventar, kein Menue-Screen -- man
    // schaltet Module mitten im Kampf um, nicht aus dem Hauptmenue heraus.
    pub(crate) module_menu_open: bool,
    // Welches Modul die rechte Spalte des Modul-Menues gerade zeigt. Rein UI-Zustand, nicht
    // persistiert -- ein neu geoeffnetes Menue zeigt wieder das erste Modul.
    pub(crate) module_menu_selected: ModuleId,
    // Welcher Hitbox-Farbeditor gerade aufgeklappt ist (hoechstens einer, s.
    // module_detail_rows). None = beide Varianten kompakt.
    // Die Zeiger-Effekte (Aufleuchten beim Druecken, Drehen beim Ziehen -- s. cursor_fx).
    pub(crate) cursor_fx: cursor_fx::CursorFx,
    // Benachrichtigungen oben rechts (s. notifications, App::notify).
    pub(crate) notifications: notifications::Notifications,
    // Angefangene Mausrad-Rasten (s. WindowEvent::MouseWheel) und wann die Lautstaerke zuletzt
    // per Rad verstellt wurde (Anzeige links, s. build_volume_display).
    pub(crate) wheel_accum: f32,
    pub(crate) volume_shown_at: Option<Instant>,
    // Wann die Anzeige zuletzt HEREINGEFAHREN ist -- nur beim ersten Erscheinen, nicht bei
    // jeder Aenderung (s. adjust_volume).
    pub(crate) volume_entered_at: Option<Instant>,
    // Gehaltene Lautstaerke-Taste (s. tick_volume_keys): Richtung und naechster Schritt, und
    // ob ein Wert noch nicht gespeichert ist.
    pub(crate) volume_key_repeat: Option<(i32, Instant)>,
    pub(crate) volume_unsaved: bool,
    // Laufender Pack-Wechsel im Hintergrund (s. reload_resource_pack), sonst None.
    pub(crate) pack_load_job: Option<PackLoadJob>,
    // Packs, die schon fuer die anderen Tabs gebaut werden oder fertig daliegen, nach dem
    // Schluessel ihres Stapels (s. pack_stack_key) -- beim Wechsel wird dann nur eingesetzt.
    pub(crate) pack_vorrat: Vec<(String, PackVorrat)>,
    // Schluessel des eingesetzten Stapels.
    pub(crate) pack_aktiv: Option<String>,
    // Zips im Skin-Ordner, die von selbst entpackt werden (s. skin_zips).
    pub(crate) skin_zips: skin_zips::SkinZips,
    // Vom Server angebotene Resourcepacks dieser Verbindung (s. server_packs).
    pub(crate) server_packs: server_packs::ServerPacks,
    // Offener Farbeditor im Schild-Modul (s. push_shield_color_rows).
    pub(crate) shield_color_editor: Option<ShieldColor>,
    // Geschaetzte Schild-Cooldowns ANDERER Spieler: Entity-Id -> restliche Ticks (s.
    // note_shield_break).
    pub(crate) shield_cooldowns: rustc_hash::FxHashMap<i32, ShieldCooldown>,
    // Flugbahnen (Trails-Modul) je Entity-Id, und der Tickzaehler, an dem ihr Alter haengt.
    pub(crate) trails: rustc_hash::FxHashMap<i32, Trail>,
    pub(crate) trail_tick: u64,
    // Erste sichtbare Detailzeile (s. module_visible_rows). Wird beim Lesen gedeckelt statt
    // beim Schreiben korrigiert -- die Zeilenzahl aendert sich auch ohne Scrollen, sobald
    // der Farbeditor auf- oder zuklappt.
    pub(crate) module_menu_scroll: smooth_scroll::SmoothScroll,
    // Welche Kategorie die Kacheln zeigen, und ob gerade die Einstellungen EINES Moduls das
    // ganze Fenster fuellen (s. module_menu_selected).
    pub(crate) module_menu_category: ModuleCategory,
    pub(crate) module_menu_detail: bool,
    // Taste, die den Chat ueber den evdev-Pfad geoeffnet hat, plus der Zeitpunkt (s.
    // apply_raw_key). Das winit-Duplikat derselben Taste wird damit einmal verworfen, statt
    // als Zeichen im Chat zu landen. Mit Zeitfenster (s. SWALLOW_TEXT_KEY_WINDOW), damit ein
    // ausgebliebenes Duplikat nicht dauerhaft ein spaeter getipptes Zeichen frisst.
    pub(crate) swallow_text_key: Option<(KeyCode, Instant)>,
    // Totem-Modul: was lag im letzten Tick in den Haenden (s. modules::TotemTracker), und
    // laeuft gerade eine Anzeige?
    pub(crate) totem_tracker: modules::TotemTracker,
    pub(crate) totem_animation: Option<modules::TotemPopAnimation>,
    // Vanillas schwebendes Pop-Totem vor dem Bildschirm (s. emit_floating_totem): restliche
    // Ticks und der zufaellige Versatz (InGameOverlayRenderer.setFloatingItem).
    pub(crate) floating_totem: Option<(u32, f32, f32)>,
    // PlayerLoaded noch offen (nach Beitreten/Respawn) -- und ob die Serverposition schon da
    // ist, an der der Chunk geprueft wird (s. tick_player_loaded).
    pub(crate) player_loaded_pending: bool,
    pub(crate) player_loaded_have_pos: bool,
    // Vanillas ClientPacketListener.clientLoaded. LocalPlayer.tick() laeuft KOMPLETT nur bei
    // hasClientLoaded() -- solange das falsch ist, gehen weder Eingabe noch Position raus.
    // Gesetzt wird es in notifyPlayerLoaded(), und zwar erst, wenn levelLoadTracker.
    // isLevelReady() gilt; tickClientLoad() braucht dafuer mindestens einen Tick.
    pub(crate) client_loaded: bool,
    // Ticks seit dem Eintreffen der Startposition -- steht fuer levelLoadTracker.tickClientLoad().
    pub(crate) load_tracker_ticks: u8,
    // Botcheck-Pass: Vanillas Ladebildschirm-Zustand (s. vanilla_level_ready). Kam das Game
    // Event INITIAL_CHUNKS_COMING schon, und seit wann laeuft das Laden (30-s-Grenze)?
    pub(crate) initial_chunks_coming: bool,
    pub(crate) world_loading_since: Instant,
    // Wann eine Spalte ihr ERSTES Mesh bekam -- Vanillas BuiltChunk merkt sich den ersten
    // Upload fuer das Einblenden (Chunk-Fade), und der Ladebildschirm wartet darauf.
    pub(crate) column_meshed_at: rustc_hash::FxHashMap<(i32, i32), Instant>,
    // Modul Chunk-Saver (s. lod.rs).
    pub(crate) lod: lod::LodState,
    // Hoehe der aktuellen Dimension (min_y steht in der Welt).
    pub(crate) world_height: i32,
    // Vom Hauptthread veroeffentlicht, vom Sofort-Ton-Haken im Netzwerk-Task gelesen (s.
    // start_connect). Drei Atomics statt eines Mutex: der Haken liegt im Lesepfad der
    // Verbindung und darf dort nichts sperren.
    //
    // Die HAND wird hier veroeffentlicht, nicht ihre Zutaten: die Entscheidung soll an genau
    // einer Stelle fallen (modules::TotemTracker::resolve, einmal pro Tick), damit Ton und
    // Anzeige nie verschiedene Haende meinen.
    pub(crate) totem_hand_published: std::sync::Arc<std::sync::atomic::AtomicU8>,
    // Was der Haken beim eigenen Pop spielt (s. toteming::OwnPopPlan). Ein Mutex, den der
    // Hauptthread nur beim Umstellen einer Einstellung kurz haelt -- der Mixer, an den der
    // Haken den Ton danach gibt, sperrt ohnehin genauso.
    pub(crate) own_pop_plan: std::sync::Arc<std::sync::Mutex<toteming::OwnPopPlan>>,
    // Woraus own_pop_plan zuletzt gebaut wurde -- neu gebaut wird nur, wenn sich davon etwas
    // aendert (s. publish_own_pop_plan). None = beim naechsten Tick neu bauen.
    pub(crate) own_pop_plan_key: Option<(toteming::TotemingSettings, f32, [f32; VOLUME_CATEGORIES.len()], bool, bool, bool)>,
    // Eigene Klangdateien fuer die Pop-Klaenge (~/.config/coral-client/sounds/).
    pub(crate) custom_pop_sounds: toteming::CustomSounds,
    // Pops je Name in dieser Verbindung (TotemPopTracker der Mod) -- fuers Log.
    pub(crate) pop_counts: std::collections::HashMap<String, u32>,
    // Crystal-Optimizer (s. crystal_optimizer.rs): hat DIESER Server abgeschaltet? Dazu die
    // Server, die es in dieser Sitzung schon getan haben (OptOutCache der Mod: die letzten
    // zehn, nur im Speicher) und die, bei denen der Hinweis schon kam. Der Hinweis selbst
    // erscheint wie in der Mod zwei Sekunden nach dem Opt-out.
    pub(crate) crystal_opted_out: bool,
    pub(crate) crystal_opt_out_servers: Vec<String>,
    // Consumable Optimizer: Warten auf den Server, vorgemerkte Klaenge, Abschaltung.
    pub(crate) consumable: consumable_optimizer::State,
    pub(crate) crystal_opt_out_notified: Vec<String>,
    pub(crate) crystal_opt_out_notice_at: Option<Instant>,
    pub(crate) own_entity_id_published: std::sync::Arc<std::sync::atomic::AtomicI32>,
    // Schwimm-/Kriech-Neigung des eigenen Spielers (Vanillas leaningPitch): laeuft in der
    // Schwimm-Pose mit 0.09 pro Tick auf 1 und genauso zurueck -- deshalb legt sich die Figur
    // weich hin statt umzuklappen. Fremde Spieler brauchen dafuer ihre Pose aus den
    // Entity-Metadaten, die dieser Client noch nicht liest (s. build_entity_vertices).
    pub(crate) own_leaning_pitch: f32,
    // Der Wert vor dem letzten Tick (Vanillas lastLeaningPitch), s. App::own_lean.
    pub(crate) own_last_leaning_pitch: f32,
    // LivingEntity.glidingTicks des eigenen Spielers (s. world::Glide).
    pub(crate) own_gliding_ticks: u32,
    // Geschwindigkeit am Ende des letzten und des vorletzten Ticks: der Gleitflug dreht die
    // Figur nach ihrer zwischen beiden interpolierten Richtung (Vanillas
    // deltaMovementOnPreviousTick().lerp(getDeltaMovement(), f), s. App::own_glide_velocity).
    pub(crate) own_tick_vel: DVec3,
    pub(crate) own_last_tick_vel: DVec3,
    // Vanilla-getreue Bewegungsphysik (Gravitation, Kollision, Sprint/Sprung) -- s.
    // physics.rs. Ersetzt die reine Fly-Kamera, sobald sich der Spieler im World-Modus
    // befindet; Fuessposition, camera.position wird daraus pro Frame abgeleitet.
    pub(crate) physics: physics::PlayerPhysics,
    // Sammelt echte Zeit auf, bis ein voller 1/20s-Physik-Tick faellig ist -- fester
    // Zeitschritt, unabhaengig von der Framerate, s. Kommentar in update_movement().
    pub(crate) physics_accum: f32,
    // Summe der an der 10-Tick-Deckelung verworfenen Zeit (s. update_movement) und der
    // Zeitpunkt der letzten Meldung darueber -- reine Diagnose fuer NegativeTimer.
    pub(crate) verworfene_tickzeit: f32,
    pub(crate) letzte_tickverlust_meldung: Instant,
    // Fuessposition VOR dem zuletzt ausgefuehrten Physik-Tick -- zusammen mit
    // physics.pos (danach) und physics_accum (Rest bis zum naechsten Tick) die Basis fuer
    // die visuelle Zwischenbild-Interpolation in update_movement(). Ohne sie wuerde die
    // Kamera bei jedem der 20 Ticks/s sichtbar "springen" statt sich bei z.B. 240 FPS
    // gleichmaessig zwischen den Tick-Positionen zu bewegen.
    pub(crate) physics_render_prev: DVec3,
    // Bloecke, die gerade ein Kolben bewegt (s. pistons.rs).
    pub(crate) pistons: pistons::Pistons,
    // GEGLAETTETE Augenhoehe der Kamera und ihr Stand vom vorigen Tick -- Vanillas
    // Camera.cameraY/lastCameraY (Yarn-1.21.11-Jar).
    //
    // Die Kamera nimmt NICHT die harte Augenhoehe der aktuellen Pose. Vanilla naehert sich ihr
    // einmal pro Tick um die Haelfte an (der alte Wert wird vorher gemerkt)
    // und interpoliert beim Zeichnen zwischen beiden mit demselben Tick-Bruchteil wie die
    // Position. Vorher sprang die Augenhoehe hier hart von 1.62 auf 1.27 (Schleichen) bzw.
    // 0.4 (Schwimmen) -- ein sichtbarer Ruck statt Vanillas weichem Abtauchen, und waehrend
    // der Annaeherung steht die Kamera mehrere Ticks lang auf einer anderen Hoehe als in
    // Vanilla.
    pub(crate) camera_eye_height: f32,
    pub(crate) camera_eye_height_prev: f32,
    // ClientPlayerEntity.sendMovementPackets()-Nachbau (Yarn-1.21.11-Jar nachgesehen):
    // welche Position/Blickrichtung zuletzt TATSAECHLICH gesendet wurde -- nur wenn sich
    // seitdem etwas geaendert hat, wird ueberhaupt (und nur das geaenderte Feld) erneut
    // gesendet. Vorher wurde JEDEN Tick bedingungslos Move Player Pos+Rot mit der aktuellen
    // Blickrichtung geschickt, auch wenn sie sich seit dem letzten Tick gar nicht geaendert
    // hatte -- von Grim als "AimDuplicateLook" geflaggter echter Bug ("sent a duplicate
    // rotation update without changing look direction").
    pub(crate) last_sent_pos: DVec3,
    pub(crate) last_sent_yaw_deg: f32,
    pub(crate) last_sent_pitch_deg: f32,
    pub(crate) last_sent_on_ground: bool,
    pub(crate) last_sent_horizontal_collision: bool,
    // ticksSinceLastPositionPacketSent: erzwingt trotz "nicht bewegt" alle 20 Ticks (1s)
    // eine Positions-Aktualisierung -- Vanillas eigener Mechanismus, damit der Server bei
    // exaktem Stillstand nicht denkt, die Verbindung haenge.
    pub(crate) ticks_since_position_sent: u32,
    // 2D-Cursor im Menu-Modus (aus rohen Maus-Deltas akkumuliert, osu!lazer-Stil).
    pub(crate) cursor_pos: (f32, f32),
    // Ob unser Fenster den Eingabefokus hat. Wichtig, weil der evdev-Raw-Input das
    // Maus-Geraet direkt liest und von X11-Fokus nichts mitbekommt -- ohne diese
    // Pruefung wuerde die Maus nach Alt-Tab weiter unsere Kamera/Cursor bewegen.
    pub(crate) focused: bool,
    pub(crate) screen: MenuScreen,
    // Laeuft gerade ein HUD-Element-Drag im HUD-Editor? Merkt sich WELCHES Element und den
    // Griff-Versatz (Cursor - Elementecke), damit das Element beim Ziehen nicht springt.
    pub(crate) hud_drag_offset: Option<(HudElement, f32, f32)>,
    // Schrift-Tab: das Kaestchen, das gerade gezogen wird.
    pub(crate) font_drag: Option<FontDrag>,
    // Wessen Einstellungen gerade offen sind (Rechtsklick auf ein Element, Fenster ueber dem
    // abgedunkelten Editor, s. build_hud_config). None = nur verschieben.
    pub(crate) hud_selected: Option<HudElement>,
    // Offener Pixel-Editor fuers Fadenkreuz (im HUD-Editor), None = zu.
    pub(crate) crosshair_painter: Option<CrosshairPainter>,
    // Ist der Spieler tot (Leben <= 0)? Erzwingt den Gestorben-Screen.
    pub(crate) is_dead: bool,
    // Wo wir gerade schlafen (LivingEntity.SLEEPING_POSITION aus den eigenen Metadaten).
    // None = wach. Der Client erfaehrt Schlafen AUSSCHLIESSLICH darueber -- ein eigenes
    // Schlaf-Paket gibt es seit 1.14 nicht mehr.
    pub(crate) sleeping_at: Option<(i32, i32, i32)>,
    pub(crate) settings: Settings,
    pub(crate) settings_status: Option<SaveStatus>,
    // Low-Latency-Audio-Engine (cpal), fuer alle Klaenge der Welt und der Oberflaeche.
    pub(crate) audio: audio::AudioEngine,
    // Womit PipeWire den Ton wirklich bedient (nur gemessen, solange der Audio-Tab offen ist).
    pub(crate) pipewire: pipewire::Status,
    // Beim Betreten des Audio-Tabs neu abgefragte Geraeteliste (s. ButtonAction::OpenScreen).
    // Nicht jeden Frame neu abfragen -- waere unnoetige Arbeit, waehrend das Menue offen ist.
    pub(crate) audio_devices: Vec<String>,
    // Die Schadensart-Registry des Servers, in Registry-Reihenfolge (s.
    // net::ClientEvent::DamageTypes). Leer, solange die Konfiguration sie nicht geschickt hat
    // -- dann gilt beim Treffer der Vanilla-Standard HURT.
    pub(crate) damage_type_effects: Vec<protocol::packets::DamageEffect>,
    // Die Verzauberungs-Registry des Servers (s. net::ClientEvent::Enchantments und
    // world::enchantment). Leer, bis die Konfiguration sie schickt.
    pub(crate) enchantments: world::enchantment::Enchantments,
    // Die Registries, die Items per Position nennen (s. protocol::packets::read_named_registry).
    pub(crate) trim_registries: TrimRegistries,
    // Das Buendel, in dem gerade per Mausrad ein Item gewaehlt ist (Menue-Slot).
    pub(crate) bundle_hover_slot: Option<usize>,
    // Zusammengesetzte Trim-Texturen im Entity-Atlas (s. assets::build_trim_texture):
    // Schluessel "<ebene>/<muster>_<palette>" -> Platz, None = gibt es nicht (Datei fehlt).
    // Dieselben Plaetze tragen auch Ruestungstexturen, die nicht im Atlas stehen (eigene
    // Equipment-Assets aus Server-Packs): Schluessel "eq:<ns>:<pfad>".
    pub(crate) trim_textures: std::collections::HashMap<String, Option<usize>>,
    pub(crate) trim_slot_order: std::collections::VecDeque<String>,
    // Equipment-Modelle nach Asset ("ns:pfad"), None = Datei fehlt (s. assets::EquipmentModel).
    pub(crate) equipment_models: std::collections::HashMap<String, Option<assets::EquipmentModel>>,
    // Sammelstelle fuer Glint-Twins (s. push_glint), waehrend eine der build_*-Funktionen
    // laeuft. Eine Zelle statt eines weiteren `out`-Parameters: die Items werden tief in
    // &self-Methoden ausgegeben (Arm -> Hand -> Item), und jede davon muesste ihn sonst
    // durchreichen. Der Aufrufer leert sie vor und nimmt sie nach jedem Bau (s. render-Pfad).
    pub(crate) glint_out: std::cell::RefCell<Vec<world::BlockVertex>>,
    // Inventar > Animation: weiche Hervorhebung und fliegende Items (s. inventory_anim). Eine
    // Zelle, weil build_inventory_screen sie beim Zeichnen weiterzaehlt.
    pub(crate) inventory_anim: std::cell::RefCell<crate::inventory_anim::InventoryAnim>,
    // Zitternder Ruestungsslot nach einem gesperrten Klick (Fluch der Bindung, s.
    // predict_inventory_click und inventory::shake_offset): Slot-Index und Startzeit.
    pub(crate) armor_shake: Option<(usize, Instant)>,
    // Regenstaerke 0..1 (s. net::ClientEvent::RainGradient); "es regnet" ab 0.2.
    pub(crate) rain_gradient: f32,
    // Gewitterstaerke 0..1 (s. net::ClientEvent::ThunderGradient), fuer sich allein noch kein
    // Gewitter: Vanilla multipliziert sie mit der Regenstaerke.
    pub(crate) thunder_gradient: f32,
    // Niederschlag + Temperatur je Biom-ID, fuer die Regenpruefung (s. world::weather).
    pub(crate) biome_climate: Vec<world::weather::BiomeClimate>,
    // coral-record: Registries und Server-Packs dieser Verbindung, kodiert wie sie ankamen
    // (true = Pack), fuer den Snapshot einer Aufnahme ab der Mitte (s. app/snapshot.rs).
    pub(crate) snapshot_config: Vec<(bool, Vec<u8>)>,
    // Textur-Property (Skin samt Signatur) je Spieler aus der Spielerliste, fuer denselben
    // Snapshot.
    pub(crate) textures_props: std::collections::HashMap<[u8; 16], String>,
    // Vanillas Klaenge, aus der lokalen Installation nachgeladen (s. vanilla_sounds).
    // Getrennt von `sounds` (der eigenen Klangbank fuer UI-Toene): das eine sind Weltgeraeusche
    // aus Mojangs Dateien, das andere unsere eigenen Aufnahmen fuer die Oberflaeche.
    pub(crate) vanilla_sounds: vanilla_sounds::VanillaSounds,
    // Laeuft gerade der Raumklang-Hoertest? (s. spatial_sound::OrbitTest -- ein Klick wandert
    // einmal um den Kopf.) Some nur waehrend der drei Sekunden, danach wieder None.
    pub(crate) orbit_test: Option<spatial_sound::OrbitTest>,
    // Laufende Weltgeraeusche, deren Lage jedes Bild nachgefuehrt wird (s.
    // update_tracked_sounds): wer sich dreht oder weggeht, hoert sie wandern und leiser werden.
    pub(crate) tracked_sounds: Vec<TrackedSound>,
    // Um wie viele Pixel die Windkugel-Textur gerade verschoben im Atlas liegt (s.
    // scroll_wind_charge_texture); None = noch nie bzw. nach einem Atlas-Neubau.
    pub(crate) wind_charge_shift: Option<u32>,
    // Laufende Jukebox-Lieder je Block (Level Event 1010/1011), fuer das gezielte Anhalten.
    pub(crate) jukeboxes: std::collections::HashMap<(i32, i32, i32), u32>,
    // Klangname je Eintrag der jukebox_song-Registry, in Registry-Reihenfolge (s. Level Event
    // 1010, dessen Datenwert der Registry-Index ist).
    pub(crate) jukebox_songs: Vec<String>,
    // Textur je Eintrag der cat_variant-Registry, in Registry-Reihenfolge; None fuer eine
    // Variante, deren Bild wir nicht kennen (dann bleibt die Standard-Katze).
    pub(crate) cat_variants: Vec<Option<&'static str>>,
    // Discord-Status (s. ModuleId::DiscordStatus) und seit wann gespielt wird.
    pub(crate) discord: discord_rpc::DiscordPresence,
    pub(crate) discord_start: u64,
    // Eigene Uhr fuer den Hoertest. Nicht die Bewegungs-Uhr (last_movement_update)
    // mitbenutzen: update_movement steigt ohne Verbindung sofort aus, der Hoertest soll aber
    // gerade im Menue ohne Server laufen koennen.
    pub(crate) last_orbit_update: Instant,
    // Some(action) waehrend im Keybinds-Tab auf einen Tastendruck gewartet wird -- der
    // naechste Tastendruck bindet die Aktion um statt normal verarbeitet zu werden (s.
    // window_event). Escape bricht ab, statt als neue Taste zu gelten.
    pub(crate) awaiting_keybind: Option<(KeyBindAction, usize)>,
    // Welche Kamera-Empfindlichkeit zuletzt verstellt wurde (true = Y). Schaltet man "X/Y
    // getrennt" aus, gilt fuer beide der ANDERE Wert (gewuenscht).
    pub(crate) sens_last_changed_y: bool,
    // Offenes Fenster "Erweitert" der Steuerungs-Seite: alle Belegungen dieser Aktion.
    pub(crate) keybind_advanced: Option<KeyBindAction>,
    // Live vom System abgefragt, nicht Teil von Settings/persistiert: TearFree ist
    // externer Systemzustand, den der Nutzer auch ausserhalb des Clients aendern kann
    // None = unbekannt/nicht AMD/noch nicht abgefragt.
    pub(crate) tearfree_state: Option<bool>,
    // Systemwert beim Start, bevor wir irgendetwas anfassen -- wird bei Fokusverlust und
    // beim Beenden wiederhergestellt. Der Schalter in den Einstellungen soll nur wirken,
    // waehrend der Client laeuft und im Vordergrund ist, nicht den Desktop dauerhaft
    // umstellen (s. Nutzeranforderung: "soll nur greifen, wenn der client laeuft und
    // full-screened ist").
    pub(crate) tearfree_original_value: Option<String>,
    // Vom Nutzer ueber den Schalter gewuenschter Wert, unabhaengig vom aktuell live am
    // System gesetzten Wert (der bei Fokusverlust auf tearfree_original_state zurueckgesetzt
    // wird). None = Nutzer hat den Schalter in dieser Sitzung noch nicht bedient.
    pub(crate) tearfree_user_choice: Option<bool>,
    // F3-Debug-Overlay (Position/Chunk/Facing/Welt-Zaehler) -- bewusst kein Settings-Feld
    // wie show_hud: rein session-lokal, wie in Vanilla nach jedem Neustart wieder aus.
    pub(crate) debug_overlay: bool,
    // Vanillas F1 (s. KeyBindAction::HideHud), ebenfalls nur fuer die Sitzung.
    pub(crate) hud_hidden: bool,
    // Zoom-Modul: Taste aktiv (s. KeyBindAction::Zoom) und der weich nachgefuehrte Anteil
    // 0..1 (s. update_zoom), samt Zeitpunkt der letzten Nachfuehrung.
    pub(crate) zoom_held: bool,
    // Sprachchat: Sprechtaste gehalten (s. KeyBindAction::PushToTalk).
    pub(crate) push_to_talk_held: bool,
    // Sprachchat-Verbindung und -Zustand (s. voicechat.rs).
    pub(crate) voice: voicechat::VoiceChat,
    // Untersuchen-Taste gehalten (s. KeyBindAction::InspectItem).
    pub(crate) inspect_held: bool,
    // Sprachchat: Fluestertaste gehalten.
    pub(crate) whisper_held: bool,
    pub(crate) zoom_amount: f32,
    pub(crate) zoom_updated: Option<Instant>,
    // Frame-Profiler (s. profiler.rs), Taste KeyBindAction::ToggleProfiler.
    pub(crate) profiler: profiler::FrameProfiler,
    // VBlank-Zeitstempel vom Kernel (s. vblank.rs) -- fuer Profiler und Riss-Steuerung, erst
    // beim ersten Bedarf gestartet (s. ensure_vblank).
    pub(crate) vblank: Option<std::sync::Arc<vblank::VblankMonitor>>,
    pub(crate) vblank_tried: bool,
    // Riss-Steuerung (s. schedule_tear_frame): naechster Praesentations-Termin, der Termin des
    // gerade angeforderten Frames und der Vorlauf (MONOTONIC-ns).
    pub(crate) tear_slot_ns: Option<i64>,
    pub(crate) tear_present_at: Option<i64>,
    pub(crate) tear_lead_ns: f64,
    // CPU/RAM/GPU-Verlauf fuer die drei Graphen im Debug-Overlay, s. sysstats.rs.
    pub(crate) sys_stats: sysstats::SystemStats,
    // Abschnitt System im Render-Tab: Speicher (hoechstens einmal je Sekunde gelesen) und ob ein
    // Compositor laeuft (alle paar Sekunden im Hintergrund abgefragt, s. system_rows).
    pub(crate) memory_info: std::cell::Cell<Option<(Instant, Option<system_info::Memory>)>>,
    pub(crate) compositing_info: std::sync::Arc<std::sync::Mutex<Option<system_info::Compositing>>>,
    pub(crate) compositing_checked: std::cell::Cell<Option<Instant>>,
    // Wie der X-Server unser Fenster gerade anzeigt (Compositor oder direkt, Flip oder Kopie),
    // nur beobachtet, solange die Render-Seite offen ist (s. system_info::watch_window,
    // update_system_watch). `window_path_tried`: dort schon versucht (kein X11 -> nicht jedes
    // Bild neu).
    pub(crate) window_path: Option<std::sync::Arc<std::sync::Mutex<system_info::WindowPath>>>,
    pub(crate) window_path_tried: bool,
    // Die letzten Log-Zeilen fuer die Verbindungsanzeige (s. push_log).
    pub(crate) log_lines: Vec<String>,
    pub(crate) conn_status: ConnStatus,
    // Wohin die laufende Verbindung wirklich geht (s. net::ClientEvent::Route), Ueberschrift des
    // Verbindungsbildschirms. None, bis die TCP-Verbindung steht.
    pub(crate) conn_route: Option<String>,
    pub(crate) events_tx: Sender<net::ClientEvent>,
    pub(crate) events_rx: Receiver<net::ClientEvent>,
    // Server-Tabs (s. app/tabs.rs); der vordere steht in den Feldern hier.
    pub(crate) tabs: Vec<app::ServerTab>,
    pub(crate) active_tab: usize,
    // Ereignisse, die der vordere Tab hinten gesammelt hat; drain_events nimmt sie zuerst.
    pub(crate) event_backlog: std::collections::VecDeque<net::ClientEvent>,
    // Gerade laeuft ein Ereignis aus event_backlog durch drain_events.
    pub(crate) replaying_backlog: bool,
    // Kanal fuer Bewegungspakete (Move Player Pos+Rot) an den Netzwerk-Task, s.
    // net::Outgoing. None = keine Verbindung (dann gibt es nichts zu schicken).
    pub(crate) outgoing_tx: Option<addons::PacketTx>,
    // Aktuelle Blickrichtung (Grad), die der Netzwerk-Task fuer die sofortige Teleport-
    // Bestaetigungs-Bewegung braucht (s. connection.rs, SharedRotation). Wird bei jedem
    // Positions-Senden aktualisiert.
    pub(crate) shared_rotation: net::SharedRotation,
    // Handle auf den laufenden Join-Task -- nur damit ist die Verbindung wieder stoppbar.
    pub(crate) conn_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) chunks_received: usize,
    // Fuer die F3-Zeile "Blick-Chunk": wann jede Spalte zuletzt kam, vergessen wurde und
    // zuletzt als unlesbares Paket ankam.
    pub(crate) column_history: std::collections::HashMap<(i32, i32), (Option<Instant>, Option<Instant>, Option<Instant>)>,
    // Letzter Welt-Reset (Weltwechsel-Respawn, Rekonfiguration) und sein Grund; die Historie
    // oben bleibt darueber stehen, damit man sieht, ob eine Spalte VOR dem Reset kam.
    pub(crate) world_reset_debug: Option<(Instant, &'static str)>,
    // Weltzustand aus den Server-Chunks; Meshes davon liegen im BlockRenderer.
    pub(crate) world: world::World,
    // Getrackte Entities (andere Spieler, Mobs, ...) aus den Server-Packets, mit
    // Interpolation zwischen den 20-Hz-Updates (s. world::entity).
    pub(crate) entities: world::Entities,
    // Fertige Chunk-Geometrie von Mesh-Worker-Threads (s. mesh_pending_columns) -- das
    // Bauen der Vertices laeuft auf tokios Blocking-Pool, nur der GPU-Upload (Empfaenger
    // dieses Kanals) muss auf diesem Thread bleiben.
    // Tupel: (opake Vertices, Wasser-Vertices -- echte Wasserbloecke + Waterlogged-
    // Fuellung, s. world::mesher-Kommentar). Getrennt, weil sie in verschiedenen
    // Render-Passes mit unterschiedlichem Blending landen (s. render::BlockRenderer).
    pub(crate) mesh_results_tx: Sender<MeshResult>,
    pub(crate) mesh_results_rx: Receiver<MeshResult>,
    // Dedizierter Worker-Pool fuer Chunk-Meshing (s. mesh_pool-Modulkommentar) -- getrennt
    // von tokio_rt, damit Settings::chunk_update_threads die Groesse live aendern kann, ohne
    // die Netzwerkverbindung zu killen.
    pub(crate) mesh_pool: mesh_pool::MeshPool,
    // Welche Spalten GERADE einen Mesh im BlockRenderer haben -- fuer die Render-Limiter (s.
    // enforce_render_limits): nur so laesst sich unterscheiden "hat noch nie einen Mesh
    // gebraucht" von "hatte einen, wurde bewusst wieder entfernt", ohne den BlockRenderer
    // selbst nach seinem Zustand fragen zu muessen.
    // FxHashSet statt std::collections::HashSet: enforce_render_limits fragt hier pro Frame
    // fuer JEDE geladene Spalte nach (bei Sichtweite 24 ~2400 Lookups), s. Kommentar an
    // rustc-hash in der Workspace-Cargo.toml.
    pub(crate) render_limit_meshed: rustc_hash::FxHashSet<(i32, i32)>,
    // Nach Abstand sortierte Spaltenliste fuer enforce_render_limits, ueber Frames gehalten:
    // (Abstand^2, cx, cz). Wird nur neu gebaut, wenn render_limit_order_key sich aendert --
    // der Puffer selbst bleibt bestehen, damit pro Frame keine Allokation anfaellt.
    pub(crate) render_limit_order: Vec<(i64, i32, i32)>,
    // (Spieler-Chunk-X, Spieler-Chunk-Z, World::column_generation) -- genau die Groessen, an
    // denen die Reihenfolge oben haengt. Aendert sich keine davon, bleibt sie gueltig.
    pub(crate) render_limit_order_key: Option<(i32, i32, u64)>,
    // Spalten mit einem GERADE LAUFENDEN Mesh-Job -- verhindert Doppel-Vergabe: enforce_
    // render_limits markiert eine Spalte JEDEN Frame erneut dirty, solange sie nicht in
    // render_limit_meshed steht (s. dortigen Kommentar), und mesh_pending_columns nimmt seit
    // take_dirty(usize::MAX) ALLES sofort. Ohne diese Sperre bekaeme eine Spalte, deren Job
    // laenger als einen Frame braucht, JEDEN Frame einen ZUSAETZLICHEN, redundanten Job --
    // bei z.B. 1000 wartenden Spalten und 60 FPS ueber ein paar Sekunden explodiert das in
    // die Millionen (gemessener Bug dieser Sitzung: F3 zeigte "5000000 im Mesh-Pool"). Mit
    // dieser Sperre bekommt jede Spalte hoechstens EINEN Job gleichzeitig; erst wenn ihr
    // Ergebnis zurueck ist (s. mesh_pending_columns-Drain), darf sie erneut eingereiht werden.
    pub(crate) mesh_jobs_pending: std::collections::HashSet<(i32, i32)>,
    // Spalten, die WAEHREND ihr Job schon lief erneut dirty markiert wurden (z.B. weil genau
    // dann ein diagonaler Nachbar eintraf, den Wasser fuer seine Eck-Hoehen braucht, s.
    // ColumnNeighbors-Kommentar in chunk.rs) -- mesh_jobs_pending schluckt diese zweite
    // Markierung sonst komplett (Ziel dort: keine doppelten Jobs), und die Spalte bliebe
    // fuer immer auf dem VERALTETEN ersten Ergebnis sitzen (Wasser-Kante/
    // Wand an Spaltengrenzen, obwohl das Wasser eigentlich verbunden ist -- der Nachbar war
    // beim ersten Meshen schlicht noch nicht da). Beim Eintreffen des ERSTEN Ergebnisses wird
    // die Spalte hieraus entfernt und sofort NEU dirty markiert, statt den zweiten Wunsch zu
    // verlieren.
    pub(crate) mesh_jobs_stale_on_completion: std::collections::HashSet<(i32, i32)>,
    // Wie viele Spalten noch zur Diagnose aufs Terminal geloggt werden (Fehlersuche).
    // Ob die Kamera schon auf die Spawn-Position gesetzt wurde. Nur das erste
    // Synchronize Player Position darf die Kamera versetzen -- spaetere Teleports
    // wuerden einen sonst mitten im Umsehen wegreissen (Fly-Kamera ist frei).
    pub(crate) camera_placed: bool,
}
