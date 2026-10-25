// Server-Tabs wie im Browser: mehrere Verbindungen in einem Client. Vorn ist immer genau ein
// Tab; sein Zustand steht wie bisher direkt in App. Die anderen liegen als TabState daneben.
// Ihre Verbindung bleibt offen, schickt aber nur KeepAlive und Pong (s. net::Events::with_background);
// was der Server ihnen schickt, sammelt sich in `backlog` und wird beim Wechsel in einem
// Rutsch verarbeitet.

use crate::*;

// Felder von App, die zu einer Verbindung gehoeren: Typ und Startwert wie in App::new.
macro_rules! tab_state {
    ($($feld:ident: $typ:ty = $start:expr,)*) => {
        pub(crate) struct TabState {
            $($feld: $typ,)*
        }

        impl TabState {
            fn leer() -> Self {
                Self { $($feld: $start,)* }
            }

            // Tauscht jedes Feld mit dem gleichnamigen in App.
            fn tauschen(&mut self, app: &mut App) {
                $(std::mem::swap(&mut self.$feld, &mut app.$feld);)*
            }
        }
    };
}

tab_state! {
    server_address: String = "127.0.0.1:25565".to_string(),
    chat_signer: Option<net::ChatSigner> = None,
    conn_status: ConnStatus = ConnStatus::Disconnected,
    conn_route: Option<String> = None,
    // Werden in TabState::new durch ein zusammengehoeriges Paar ersetzt.
    events_tx: Sender<net::ClientEvent> = std::sync::mpsc::channel().0,
    events_rx: Receiver<net::ClientEvent> = std::sync::mpsc::channel().1,
    outgoing_tx: Option<addons::PacketTx> = None,
    shared_rotation: net::SharedRotation = std::sync::Arc::new(net::SharedLook::default()),
    conn_task: Option<tokio::task::JoinHandle<()>> = None,
    log_lines: Vec<String> = Vec::new(),
    chunks_received: usize = 0,
    column_history: std::collections::HashMap<(i32, i32), (Option<Instant>, Option<Instant>, Option<Instant>)> = std::collections::HashMap::new(),
    world_reset_debug: Option<(Instant, &'static str)> = None,
    transactions: transactions::Transactions = transactions::Transactions::default(),
    last_server_event: Instant = Instant::now(),
    session: session_info::Session = session_info::Session::new(Instant::now()),
    screen: MenuScreen = MenuScreen::Connect,
    disconnect_popup: Option<String> = None,
    dialog: Option<dialog_screen::OpenDialog> = None,
    dialog_waiting: Option<Instant> = None,
    code_of_conduct: Option<dialog_screen::CodeOfConduct> = None,
    server_links: Vec<protocol::packets::ServerLink> = Vec::new(),
    dialog_registry: Vec<(String, Option<protocol::nbt::NbtValue>)> = Vec::new(),
    team_color_log: std::collections::HashMap<[u8; 16], String> = std::collections::HashMap::new(),
    team_facts: std::collections::HashMap<[u8; 16], teams::Facts> = std::collections::HashMap::new(),
    team_color_memory: std::collections::HashMap<[u8; 16], u32> = std::collections::HashMap::new(),
    chat_messages: std::collections::VecDeque<Vec<protocol::nbt::TextSpan>> = std::collections::VecDeque::new(),
    chat_times: std::collections::VecDeque<Instant> = std::collections::VecDeque::new(),
    chat_scroll: usize = 0,
    chat_dropped_lines: usize = 0,
    action_bar: Option<(Vec<protocol::nbt::TextSpan>, Instant)> = None,
    title_text: Option<Vec<protocol::nbt::TextSpan>> = None,
    subtitle_text: Option<Vec<protocol::nbt::TextSpan>> = None,
    title_set_at: Instant = Instant::now(),
    title_fade_in: i32 = 10,
    title_stay: i32 = 70,
    title_fade_out: i32 = 20,
    sb_objective_titles: std::collections::HashMap<String, String> = std::collections::HashMap::new(),
    sb_sidebar_objective: Option<String> = None,
    sb_scores: std::collections::HashMap<String, std::collections::HashMap<String, (i32, Option<String>)>> = std::collections::HashMap::new(),
    sb_teams: std::collections::HashMap<String, TeamInfo> = std::collections::HashMap::new(),
    sb_entry_team: std::collections::HashMap<String, String> = std::collections::HashMap::new(),
    tab_entries: std::collections::HashMap<[u8; 16], TabEntry> = std::collections::HashMap::new(),
    tab_header: Vec<protocol::nbt::TextSpan> = Vec::new(),
    tab_footer: Vec<protocol::nbt::TextSpan> = Vec::new(),
    sb_list_objective: Option<String> = None,
    sb_objective_hearts: std::collections::HashSet<String> = std::collections::HashSet::new(),
    sb_objective_formats: std::collections::HashMap<String, protocol::packets::NumberFormat> = std::collections::HashMap::new(),
    sb_score_formats: std::collections::HashMap<(String, String), protocol::packets::NumberFormat> = std::collections::HashMap::new(),
    boss_bars: Vec<BossBarState> = Vec::new(),
    player_names: std::collections::HashMap<[u8; 16], String> = std::collections::HashMap::new(),
    dimension_types: Vec<protocol::packets::DimensionInfo> = Vec::new(),
    dimension_type_name: String = String::new(),
    biome_names: Vec<String> = Vec::new(),
    biome_climate: Vec<world::weather::BiomeClimate> = Vec::new(),
    biome_palette: std::sync::Arc<world::BiomePalette> = std::sync::Arc::new(world::BiomePalette::fallback()),
    damage_type_effects: Vec<protocol::packets::DamageEffect> = Vec::new(),
    enchantments: world::enchantment::Enchantments = world::enchantment::Enchantments::default(),
    trim_registries: TrimRegistries = TrimRegistries::default(),
    jukebox_songs: Vec<String> = Vec::new(),
    cat_variants: Vec<Option<&'static str>> = Vec::new(),
    creative_registries: creative::Registries = creative::Registries::default(),
    world: world::World = world::World::new(),
    entities: world::Entities = world::Entities::new(),
    pistons: pistons::Pistons = pistons::Pistons::default(),
    block_lids: std::collections::HashMap<(i32, i32, i32), BlockLid> = std::collections::HashMap::new(),
    signs: std::collections::HashMap<(i32, i32, i32), protocol::sign::SignText> = std::collections::HashMap::new(),
    placed_heads: std::collections::HashMap<(i32, i32, i32), [u8; 16]> = std::collections::HashMap::new(),
    maps: world::map::Maps = world::map::Maps::default(),
    jukeboxes: std::collections::HashMap<(i32, i32, i32), u32> = std::collections::HashMap::new(),
    block_sequence: i32 = 0,
    pending_blocks: Vec<PendingBlock> = Vec::new(),
    world_height: i32 = 384,
    world_time_of_day: i64 = 6000,
    world_time_ticking: bool = true,
    sky_time_pending: bool = false,
    sky_world_pending: Option<Instant> = None,
    rain_gradient: f32 = 0.0,
    thunder_gradient: f32 = 0.0,
    initial_chunks_coming: bool = false,
    world_loading_since: Instant = Instant::now(),
    player_loaded_pending: bool = false,
    player_loaded_have_pos: bool = false,
    client_loaded: bool = false,
    load_tracker_ticks: u8 = 0,
    camera_placed: bool = false,
    dying: std::collections::HashMap<i32, u32> = std::collections::HashMap::new(),
    kinetic_hit_at: std::collections::HashMap<i32, Instant> = std::collections::HashMap::new(),
    shield_cooldowns: rustc_hash::FxHashMap<i32, ShieldCooldown> = rustc_hash::FxHashMap::default(),
    item_cooldowns: item_cooldowns::ItemCooldowns = item_cooldowns::ItemCooldowns::default(),
    inventory: Inventory = Inventory::default(),
    recipe_book: recipe_book::RecipeBook = recipe_book::RecipeBook::default(),
    physics: physics::PlayerPhysics = physics::PlayerPhysics::new(DVec3::new(8.0, 10.0, -10.0)),
    camera: render::Camera = render::Camera::new(DVec3::new(8.0, 10.0, -10.0)),
    health: f32 = 20.0,
    max_health: f32 = 20.0,
    armor: i32 = 0,
    absorption: f32 = 0.0,
    xp_level: i32 = 0,
    xp_progress: f32 = 0.0,
    air_ticks: i32 = MAX_AIR_TICKS,
    food_level: i32 = 20,
    food_saturation: f32 = 5.0,
    own_effects: std::collections::HashMap<i32, i32> = std::collections::HashMap::new(),
    shown_effects: std::collections::HashMap<i32, ShownEffect> = std::collections::HashMap::new(),
    game_mode: u8 = 0,
    previous_game_mode: Option<u8> = None,
    op_level: u8 = 0,
    attr_block_break_speed: f32 = 1.0,
    attr_mining_efficiency: f32 = 0.0,
    attr_submerged_mining_speed: f32 = 0.2,
    attr_movement_speed: f64 = physics::BASE_SPEED,
    attr_attack_speed: Option<(f64, f64, f64)> = None,
    attr_sneaking_speed: f64 = physics::DEFAULT_SNEAKING_SPEED,
    attr_water_movement_efficiency: f64 = 0.0,
    attr_body: physics::BodyAttributes = physics::BodyAttributes::default(),
    attr_block_range: f64 = BLOCK_INTERACTION_REACH,
    attr_entity_range: f64 = ENTITY_INTERACTION_REACH,
    own_entity_id: Option<i32> = None,
    own_uuid: Option<[u8; 16]> = None,
    own_ride: Option<vehicle::OwnRide> = None,
    ride_yaw_spin: (f32, f32) = (0.0, 0.0),
    is_dead: bool = false,
    sleeping_at: Option<(i32, i32, i32)> = None,
    camera_entity: Option<i32> = None,
    spectator_flight: Option<DVec3> = None,
    last_sent_pos: DVec3 = DVec3::new(8.0, 10.0, -10.0),
    last_sent_yaw_deg: f32 = 0.0,
    last_sent_pitch_deg: f32 = 0.0,
    last_sent_on_ground: bool = false,
    last_sent_horizontal_collision: bool = false,
    last_sent_sprinting: Option<bool> = Some(false),
    last_sent_player_input: Option<(bool, bool, bool, bool, bool, bool, bool)> = Some((false, false, false, false, false, false, false)),
    ticks_since_position_sent: u32 = 0,
    last_synced_slot: usize = 0,
    pending_input_actions: Vec<PendingInputAction> = Vec::new(),
    slot_queue: SlotQueue = SlotQueue::default(),
    consumable: consumable_optimizer::State = consumable_optimizer::State::default(),
    crystal_opted_out: bool = false,
    totem_tracker: modules::TotemTracker = modules::TotemTracker::new(),
    pop_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new(),
    own_entity_id_published: std::sync::Arc<std::sync::atomic::AtomicI32> = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(-1)),
    skin_sources: std::collections::HashMap<[u8; 16], auth::SkinRef> = std::collections::HashMap::new(),
    head_textures: std::collections::HashMap<[u8; 16], Option<auth::SkinRef>> = std::collections::HashMap::new(),
    // Koepfe der Tab-Liste samt Pixeln: beim Wechsel wieder in ihre Plaetze geschrieben.
    tab_heads: std::collections::HashMap<[u8; 16], (usize, Vec<u8>)> = std::collections::HashMap::new(),
    tab_head_order: std::collections::VecDeque<([u8; 16], usize)> = std::collections::VecDeque::new(),
    own_limbs: world::LimbAnimator = world::LimbAnimator::new(),
    own_age_ticks: f32 = 0.0,
    own_hurt_ticks: u8 = 0,
    own_ticks_since_hurt: u32 = u32::MAX,
    own_hurt_by_crit: bool = false,
    own_on_fire: bool = false,
    own_frozen_ticks: i32 = 0,
    hardcore: bool = false,
    own_potion_swirls: Vec<u32> = Vec::new(),
    own_potion_swirls_ambient: bool = false,
    own_invisible: bool = false,
    own_glowing: bool = false,
    own_crit_grace: u8 = 0,
    own_health_initialized: bool = false,
    own_body_yaw: world::BodyYaw = world::BodyYaw::new(0.0),
    own_render_yaw: f32 = 0.0,
    own_render_pitch: f32 = 0.0,
    own_last_render_yaw: f32 = 0.0,
    own_last_render_pitch: f32 = 0.0,
    own_leaning_pitch: f32 = 0.0,
    own_gliding_ticks: u32 = 0,
    physics_render_prev: DVec3 = DVec3::new(8.0, 10.0, -10.0),
    camera_eye_height: f32 = physics::EYE_HEIGHT,
    camera_eye_height_prev: f32 = physics::EYE_HEIGHT,
    own_use_ticks: u32 = 0,
    own_equip_progress: f32 = 0.0,
    own_last_equip_progress: f32 = 0.0,
    own_swing_ticks: u32 = 0,
    own_cape: world::CapeState = world::CapeState::new(),
}

impl TabState {
    fn new() -> Self {
        let mut t = Self::leer();
        (t.events_tx, t.events_rx) = std::sync::mpsc::channel();
        t
    }
}

pub(crate) struct ServerTab {
    // None beim Tab vorn: sein Zustand steht in App.
    state: Option<Box<TabState>>,
    // Was der Server geschickt hat, waehrend der Tab hinten lag.
    backlog: std::collections::VecDeque<net::ClientEvent>,
    // Jemand hat einen im Chat erwaehnt, waehrend der Tab hinten lag.
    pub(crate) mention: bool,
    // Geht an die Verbindung (s. net::Events::with_background).
    pub(crate) background: std::sync::Arc<std::sync::atomic::AtomicBool>,
    // Die Fernsicht-Welt (s. lod) und die Server-Packs dieses Tabs, solange er hinten liegt.
    lod_identity: Option<(String, i64)>,
    server_packs: server_packs::ServerPacks,
}

impl ServerTab {
    pub(crate) fn new() -> Self {
        Self {
            state: None,
            backlog: std::collections::VecDeque::new(),
            mention: false,
            background: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            lod_identity: None,
            server_packs: server_packs::ServerPacks::default(),
        }
    }
}

// Breite eines Tabs in der Leiste oben, ohne das Plus.
const TAB_MAX_W: f32 = 220.0;
const TAB_MIN_W: f32 = 90.0;
const TAB_GAP: f32 = 4.0;
const TAB_CLOSE: f32 = 18.0;

// Ein Tab in der Leiste: ganze Flaeche und das Kreuz darin.
pub(crate) struct TabRect {
    pub(crate) tab: Rect,
    pub(crate) close: Rect,
}

impl App {
    // Neuer Tab mit der Serverliste, sofort vorn.
    pub(crate) fn new_tab(&mut self) {
        let mut tab = ServerTab::new();
        tab.state = Some(Box::new(TabState::new()));
        tab.background.store(true, std::sync::atomic::Ordering::Relaxed);
        self.tabs.push(tab);
        self.switch_tab(self.tabs.len() - 1);
    }

    pub(crate) fn switch_tab(&mut self, i: usize) {
        if i == self.active_tab || i >= self.tabs.len() {
            return;
        }
        let alt = self.active_tab;

        // Was am alten Tab auf der Platte oder in geteilten Systemen haengt.
        self.tabs[alt].lod_identity = self.lod_leave_tab();
        self.voice.reset();
        self.clear_controls();

        let Some(mut state) = self.tabs[i].state.take() else { return };
        state.tauschen(self);
        self.tabs[alt].state = Some(state);
        let mut packs = std::mem::take(&mut self.tabs[i].server_packs);
        self.server_packs.switch_tab(&mut self.tabs[alt].server_packs, &mut packs);
        self.tabs[alt].background.store(true, std::sync::atomic::Ordering::Relaxed);
        self.tabs[i].background.store(false, std::sync::atomic::Ordering::Relaxed);
        self.event_backlog = std::mem::take(&mut self.tabs[i].backlog);
        self.tabs[i].mention = false;
        self.active_tab = i;

        self.forget_view_caches();
        if let Some((dimension, seed)) = self.tabs[i].lod_identity.take() {
            self.lod_world_identity(dimension, seed);
        }
        // Verbunden: gleich in die Welt, das Pausemenue des Tabs zu. Sonst der Bildschirm, den
        // der Tab zuletzt hatte (Serverliste, Todesbildschirm).
        if self.conn_status != ConnStatus::Disconnected && self.screen != MenuScreen::Death {
            self.screen = MenuScreen::Main;
            self.mode = AppMode::World;
            self.restore_held_controls();
        } else {
            self.mode = AppMode::Menu;
        }
        // Die Koepfe der Konten oben rechts standen im geleerten Skin-Atlas.
        self.request_account_heads();
        self.update_discord_status();
    }

    // Schliesst einen Tab. Der letzte bleibt stehen und wird nur getrennt.
    pub(crate) fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            self.disconnect();
            return;
        }
        if i == self.active_tab {
            self.disconnect();
            self.switch_tab(if i + 1 < self.tabs.len() { i + 1 } else { i - 1 });
        }
        let tab = self.tabs.remove(i);
        if let Some(task) = tab.state.and_then(|mut s| s.conn_task.take()) {
            task.abort();
        }
        if i < self.active_tab {
            self.active_tab -= 1;
        }
    }

    // Jeden Frame: die Ereignisse der hinteren Tabs abholen, damit sich nichts im Kanal
    // staut, und Erwaehnungen markieren.
    pub(crate) fn poll_background_tabs(&mut self) {
        let name = self.credentials.as_ref().map(|c| c.username.clone()).unwrap_or_default();
        for tab in &mut self.tabs {
            let Some(state) = tab.state.as_ref() else { continue };
            while let Ok(event) = state.events_rx.try_recv() {
                if let net::ClientEvent::Chat(spans, false) = &event {
                    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
                    if !name.is_empty() && text.contains(&name) && !text.starts_with(&format!("<{name}>")) {
                        tab.mention = true;
                    }
                }
                tab.backlog.push_back(event);
            }
        }
    }

    // Was auf der GPU oder in Caches dazu nur fuer den alten Tab stand: Chunk-Meshes, Skins
    // und Koepfe im Atlas, Karten, Items in Haenden. Alles baut sich aus dem neuen Zustand
    // wieder auf.
    fn forget_view_caches(&mut self) {
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.block_renderer.clear_meshes();
            for (slot, pixel) in self.tab_heads.values() {
                if let Some(origin) = self.entity_atlas.head_slot_origin(*slot) {
                    gpu.block_renderer.upload_item(&gpu.queue, origin, assets::HEAD_SLOT_WIDTH, assets::HEAD_SLOT_HEIGHT, pixel);
                }
            }
        }
        // Noch laufende Mesh-Auftraege gehoeren zur alten Welt: neuer Kanal, ihre Ergebnisse
        // landen im Leeren.
        (self.mesh_results_tx, self.mesh_results_rx) = std::sync::mpsc::channel();
        self.mesh_jobs_pending.clear();
        self.mesh_jobs_stale_on_completion.clear();
        self.render_limit_meshed.clear();
        self.render_limit_order.clear();
        self.render_limit_order_key = None;
        self.column_meshed_at.clear();
        self.column_light.clear();
        self.ao_fades.clear();
        self.ao_fade_sources.clear();
        self.ghosts_awaiting_mesh.clear();
        let spalten: Vec<(i32, i32)> = self.world.column_keys().collect();
        for (x, z) in spalten {
            self.world.mark_dirty(x, z);
        }

        self.skins.clear();
        self.skin_slot_order.clear();
        self.cape_slot_order.clear();
        self.head_lookups.clear();
        self.requested_skins.clear();
        self.skin_waiters.clear();
        self.skin_retries.clear();
        self.held_items.clear();
        self.map_slots.clear();
        self.particles.clear();
        self.tracked_sounds.clear();

        self.chat_generation += 1;
        self.chat_view.set(None);
    }

    // Beschriftung eines Tabs: Name aus der Serverliste, sonst die Adresse; ohne Verbindung
    // "Serverliste".
    pub(crate) fn tab_title(&self, i: usize) -> String {
        let (adresse, status) = match self.tabs.get(i).and_then(|t| t.state.as_ref()) {
            Some(s) => (&s.server_address, s.conn_status),
            None => (&self.server_address, self.conn_status),
        };
        if status == ConnStatus::Disconnected {
            return "Serverliste".to_string();
        }
        // Eintraege ohne eigenen Namen zeigen in der Liste auch nur die Adresse.
        self.servers
            .iter()
            .find(|s| s.address.eq_ignore_ascii_case(adresse) && !s.name.trim().is_empty())
            .map_or_else(|| adresse.clone(), |s| s.name.clone())
    }

    // Tabs in der Leiste oben, zwischen Zahnrad und Konto, und dahinter das Plus.
    pub(crate) fn tab_rects(&self, l: &TopbarLayout) -> (Vec<TabRect>, Rect) {
        let y = 8.0;
        let h = TOPBAR_H - y;
        let links = l.gear_hit.x + l.gear_hit.w + 12.0;
        let rechts = l.konto_links - 16.0 - h;
        let n = self.tabs.len() as f32;
        let w = ((rechts - links - TAB_GAP * n) / n).clamp(TAB_MIN_W, TAB_MAX_W).floor();
        let tabs: Vec<TabRect> = (0..self.tabs.len())
            .map(|i| {
                let tab = Rect { x: links + i as f32 * (w + TAB_GAP), y, w, h };
                let close = Rect { x: tab.x + tab.w - TAB_CLOSE - 8.0, y: tab.y + ((tab.h - TAB_CLOSE) / 2.0).round(), w: TAB_CLOSE, h: TAB_CLOSE };
                TabRect { tab, close }
            })
            .collect();
        let plus_x = links + n * (w + TAB_GAP);
        (tabs, Rect { x: plus_x, y: y + ((h - (h - 8.0)) / 2.0).round(), w: h - 8.0, h: h - 8.0 })
    }

    pub(crate) fn build_tab_bar(&self, gpu: &GpuState, l: &TopbarLayout, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        use theme::Slot;
        let g = theme::get;
        let (cx, cy) = self.cursor_pos;
        let (tabs, plus) = self.tab_rects(l);
        for (i, r) in tabs.iter().enumerate() {
            let vorn = i == self.active_tab;
            let hover = r.tab.contains(cx, cy);
            let farbe = if vorn {
                g(Slot::Surface3)
            } else if hover {
                g(Slot::Surface2)
            } else {
                g(Slot::Surface1)
            };
            // Nur die oberen Ecken rund: die unteren 8 Pixel eckig ueberdeckt, Rand seitlich
            // weitergezogen. Endet genau an der Unterkante der Leiste.
            let rand = g(Slot::Border1);
            let unten = Rect { y: r.tab.y + r.tab.h - 8.0, h: 8.0, ..r.tab };
            rects.push(UiRect::framed(r.tab, farbe, 8.0, 1.0, rand));
            rects.push(UiRect { rect: Rect { x: unten.x + 1.0, w: unten.w - 2.0, ..unten }, color: farbe, ..UiRect::FLAT });
            rects.push(UiRect { rect: Rect { w: 1.0, ..unten }, color: rand, ..UiRect::FLAT });
            rects.push(UiRect { rect: Rect { x: unten.x + unten.w - 1.0, w: 1.0, ..unten }, color: rand, ..UiRect::FLAT });
            if vorn {
                rects.push(UiRect { rect: Rect { x: r.tab.x + 1.0, y: r.tab.y + r.tab.h - 3.0, w: r.tab.w - 2.0, h: 3.0 }, color: g(Slot::Accent), ..UiRect::FLAT });
            }
            let mut text_x = r.tab.x + 12.0;
            if self.tabs[i].mention {
                rects.push(UiRect::rounded(Rect { x: text_x, y: r.tab.y + (r.tab.h - 8.0) / 2.0, w: 8.0, h: 8.0 }, g(Slot::Accent), 4.0));
                text_x += 14.0;
            }
            let platz = r.close.x - 6.0 - text_x;
            let titel = ellipsize(gpu, &self.tab_title(i), UI_SCALE_SMALL, platz);
            let ty = (r.tab.y + (r.tab.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: titel, x: text_x, y: ty, color: theme::text(), scale: UI_SCALE_SMALL });
            let ueber_kreuz = r.close.contains(cx, cy);
            if ueber_kreuz {
                rects.push(UiRect::rounded(r.close, g(Slot::Surface3), 4.0));
            }
            push_close_icon(rects, r.close, if ueber_kreuz { theme::text() } else { theme::text_2() });
        }
        let hover = plus.contains(cx, cy);
        if hover {
            rects.push(UiRect::rounded(plus, g(Slot::Surface2), 6.0));
        }
        push_plus_icon(rects, plus, if hover { theme::text() } else { theme::text_2() });
    }
}

// Kreuz zum Schliessen, mittig in `r`.
fn push_close_icon(rects: &mut Vec<UiRect>, r: Rect, color: [f32; 4]) {
    let (cx, cy, h) = (r.x + r.w / 2.0, r.y + r.h / 2.0, 4.0);
    push_bar(rects, (cx - h, cy - h), (cx + h, cy + h), 1.8, color);
    push_bar(rects, (cx - h, cy + h), (cx + h, cy - h), 1.8, color);
}

fn push_plus_icon(rects: &mut Vec<UiRect>, r: Rect, color: [f32; 4]) {
    let (cx, cy, h) = (r.x + r.w / 2.0, r.y + r.h / 2.0, 7.0);
    push_bar(rects, (cx - h, cy), (cx + h, cy), 2.0, color);
    push_bar(rects, (cx, cy - h), (cx, cy + h), 2.0, color);
}
