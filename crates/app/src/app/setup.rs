// App::new und der Aufbau beim Start.

use crate::*;

impl App {
    // `assets`: schon gebaut (s. loading_screen -- das dauert, und solange zeigt das Fenster den
    // Ladebildschirm).
    pub(crate) fn new(mut installation: assets::MinecraftInstallation, settings: Settings, assets: WorldAssets) -> Self {
        let (status_tx, status_rx) = std::sync::mpsc::channel();
        // "Sprinten an bei Spielstart": der Umschalt-Zustand startet eingeschaltet, als waere
        // die Taste einmal gedrueckt worden. Mit "Halten" gibt es keinen solchen Zustand.
        let sprint_on_start = settings.sprint_on_start && settings.keybind_modes.sprint == KeybindMode::Toggle;
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let (auth_tx, auth_rx) = std::sync::mpsc::channel();
        let (mesh_results_tx, mesh_results_rx) = std::sync::mpsc::channel();

        // Sprachdateien fuer "translate"-Texte (Itemnamen, Effekte, Todesnachrichten ...) und
        // die Sprache der Oberflaeche, s. apply_language_settings.
        apply_language_settings(&mut installation, &settings);
        let custom_sky = custom_sky::CustomSky::new(custom_sky::load_defs(&mut installation));

        let WorldAssets {
            atlas,
            ui_atlas,
            item_icons,
            entity_atlas,
            particle_atlas,
            entity_models,
            player_wide,
            player_slim,
            cape_model,
            alt_font,
            bed_head_model,
            bed_foot_model,
            armor_models,
            shield,
            grass_colormap,
            foliage_colormap,
            dry_foliage_colormap,
            block_uvs,
            block_shapes,
            cross_blocks,
            cross_extra_shapes,
        } = assets;
        let biome_palette = std::sync::Arc::new(world::BiomePalette::fallback());

        // block_uvs' Umschalter-Zustand ist Laufzeitzustand (AtomicBool, s. Feldkommentar) --
        // hier einmal mit dem geladenen/Default-Settingswert synchronisieren, sonst bliebe er
        // beim harten Default (an) haengen, egal was in der Konfigdatei steht.
        block_uvs.set_rng_rotation_enabled(settings.block_rng_rotation);
        let mut audio = audio::AudioEngine::with_buffer_frames(audio_buffer_request(settings.audio_buffer_frames));
        if !settings.audio_output_device.is_empty() {
            if let Err(e) = audio.set_output_device(Some(&settings.audio_output_device)) {
                // Gespeichertes Geraet nicht mehr da (z.B. USB-Headset ab) -- Engine bleibt
                // auf dem Standardgeraet, das new() schon gestartet hat, statt stumm zu sein.
                eprintln!("[audio] gespeichertes Ausgabegerät nicht verfügbar: {e}");
            }
        }
        let audio_devices = audio::list_output_devices();
        // Nur der INDEX (sounds.json) wird jetzt gelesen; die eigentlichen Aufnahmen kommen
        // erst dann vom Datentraeger, wenn sie zum ersten Mal gebraucht werden. 4405 Dateien
        // beim Start zu dekodieren waere eine lange Wartezeit fuer Klaenge, von denen in einer
        // Sitzung nur eine Handvoll vorkommt.
        let vanilla_sounds = vanilla_sounds::VanillaSounds::load(&mut installation);
        let mesh_pool = mesh_pool::MeshPool::new(settings.chunk_update_threads);
        let particles = particles::Particles::new(&particle_atlas, atlas.particle_uvs.iter().map(|(k, v)| (k.clone(), *v)).collect());
        let (raw_mouse, raw_keyboard, input_watcher) = raw_input::open();
        // input-log: Ziel ist eine Datei neben den uebrigen Client-Dateien; Zustand folgt der
        // gespeicherten Einstellung (Vorgabe aus). Geoeffnet wird erst beim ersten Ereignis.
        let mut input_log = input_log::InputLog::new(shared_config_dir().map(|d| d.join("input_log.txt")));
        input_log.set_enabled(settings.module_input_log_enabled);
        // coral-record: Aufnahmen neben den uebrigen Client-Dateien.
        if let Some(d) = shared_config_dir() {
            record::set_dir(d.join("recordings"));
        }
        record::set_client_version(concat!("v", env!("CARGO_PKG_VERSION")).to_string());
        record::set_enabled(settings.module_coral_record);
        let mut item_icons = ItemIcons::new(item_icons);
        set_shield_icon(&mut item_icons, &shield, &entity_atlas);
        set_special_icons(&mut item_icons, &entity_atlas, &mut installation);

        Self {
            gpu: None,
            atlas_mips: atlas.mips,
            atlas_size: atlas.size,
            ui_atlas,
            item_icons,
            entity_atlas,
            particle_atlas,
            particles,
            particle_vertices: Vec::new(),
            particle_prev: particles::ParticlePrev::default(),
            particle_prev_frame: None,
            precipitation: precipitation::Precipitation::new(),
            weather_vertices: Vec::new(),
            weather_textures: load_weather_textures(&mut installation),
            weather_textures_uploaded: None,
            weather_layouts: [precipitation::SheetLayout { rows: 0, offsets: [[0.0; 2]; 20] }; 2],
            hud_latched_widths: Default::default(),
            held_items: held_item::HeldItems::new(),
            last_inventory_click: None,
            last_inventory_click_at: Instant::now(),
            inventory_drag: None,
            quick_craft_pending: None,
            inventory_move_stack_held: false,
            player_list_held: false,
            player_list_rect: Default::default(),
            entity_models,
            player_wide,
            player_slim,
            cape_model,
            alt_font,
            book_model: world::entity_model::enchanting_book_model(),
            book_anim: BookAnim::default(),
            bed_head_model,
            bed_foot_model,
            armor_models,
            shield,
            inventory: Inventory::default(),
            recipe_book: recipe_book::RecipeBook::default(),
            water_animation_frames: atlas.water_animation_frames,
            water_tile_origin: atlas.water_tile_origin,
            water_flow_animation_frames: atlas.water_flow_animation_frames,
            water_flow_tile_origin: atlas.water_flow_tile_origin,
            water_tile_size: atlas.tile_size,
            water_anim_frame: 0,
            water_anim_last_switch: Instant::now(),
            block_animations: block_anim::BlockAnimations::new(atlas.animations),
            entity_vertex_hint: 0,
            fire_alpha: 1.0,
            block_anim_start: Instant::now(),
            ui_anim_last: Vec::new(),
            block_uvs,
            block_shapes,
            cross_blocks,
            cross_extra_shapes,
            grass_colormap,
            foliage_colormap,
            dry_foliage_colormap,
            biome_palette,
            frame_stats: FrameStats::default(),
            loop_idle_since: None,
            entity_update_stats: EntityUpdateStats::default(),
            damage_type_effects: Vec::new(),
            enchantments: world::enchantment::Enchantments::default(),
            trim_registries: TrimRegistries::default(),
            bundle_hover_slot: None,
            trim_textures: std::collections::HashMap::new(),
            trim_slot_order: std::collections::VecDeque::new(),
            equipment_models: std::collections::HashMap::new(),
            glint_out: std::cell::RefCell::new(Vec::new()),
            inventory_anim: std::cell::RefCell::default(),
            hotbar_glide: std::cell::RefCell::default(),
            entity_motion: crate::entity_motion::EntityMotion::default(),
            block_motion: crate::entity_motion::EntityMotion::default(),
            orb_prev_centers: std::collections::HashMap::new(),
            armor_shake: None,
            rain_gradient: 0.0,
            thunder_gradient: 0.0,
            biome_climate: Vec::new(),
            snapshot_config: Vec::new(),
            textures_props: std::collections::HashMap::new(),
            vanilla_sounds,
            installation,
            custom_sky,
            biome_names: Vec::new(),
            monitor_frame_interval: FALLBACK_FRAME_INTERVAL,
            next_frame_time: Instant::now(),
            ticking: false,
            tick_deadline: None,
            menu_world_frozen: false,
            config_delete_armed: false,
            particle_scroll: Default::default(),
            particle_screen_own: false,
            skin_scroll: Default::default(),
            start_time: Instant::now(),
            raw_mouse,
            raw_keyboard,
            caps_lock: caps_lock::CapsLockGuard::default(),
            caps_lock_recheck: None,
            input_watcher,
            input_log,
            recorder: app::AppRecorder::default(),
            recordings_open: false,
            recordings: Vec::new(),
            replay: None,
            autoclick: Autoclick::default(),
            session: session_info::Session::new(Instant::now()),
            dying: std::collections::HashMap::new(),
            death_bursts: Vec::new(),
            wallpaper_loader: wallpaper::Loader::default(),
            wallpaper_checked_at: Instant::now(),
            last_view_requested: false,
            last_view_rx: None,
            connect_view_for: None,
            connect_view_rx: None,
            connect_view_taken: false,
            connect_view_first_at: None,
            hero_building: None,
            hero_done: false,
            hero_fov: None,
            startup_window: None,
            hero_render_distance: None,
            tool_break_notified_at: None,
            server_address: "127.0.0.1:25565".to_string(),
            // Gespeicherte Anmeldung wiederherstellen (bleibt ueber Neustarts erhalten). Ist das
            // Token abgelaufen, meldet der naechste Connect einen klaren Login-Fehler.
            credentials: (!settings.mc_access_token.is_empty()).then(|| auth::Credentials {
                access_token: settings.mc_access_token.clone(),
                profile_id: settings.account_uuid.clone(),
                username: settings.account_name.clone(),
            }),
            auth_tx,
            auth_rx,
            login_busy: false,
            device_login: None,
            input_field: None,
            servers: load_servers(),
            server_context: None,
            server_drag: None,
            server_scroll: Default::default(),
            server_bar_grab: None,
            server_search: None,
            server_search_applied: String::new(),
            ui_anim_at: Instant::now(),
            quick_connect_open: false,
            theme_name_dialog: None,
            theme_delete: None,
            disconnect_popup: None,
            dialog: None,
            dialog_waiting: None,
            link_pending: None,
            code_of_conduct: None,
            server_links: Vec::new(),
            dialog_registry: Vec::new(),
            team_color_log: std::collections::HashMap::new(),
            team_facts: std::collections::HashMap::new(),
            team_color_memory: std::collections::HashMap::new(),
            quick_field: text_field::TextField::new("", MAX_ADDRESS_LEN),
            quick_dragging: false,
            quick_last_press: None,
            account_menu_open: false,
            account_manager_open: false,
            server_status: std::collections::HashMap::new(),
            server_status_tx: status_tx,
            server_status_rx: status_rx,
            next_icon_slot: 0,
            unknown_server_icon: false,
            server_edit: None,
            accounts: load_account_store(&settings),
            account_checks: std::collections::HashMap::new(),
            account_context: None,
            // Token-Export auskommentiert, vorerst abgeschaltet.
            // token_export: None,
            chat_input: None,
            chat_history: load_chat_history(),
            chat_messages: std::collections::VecDeque::new(),
            chat_times: std::collections::VecDeque::new(),
            block_lids: std::collections::HashMap::new(),
            chat_generation: 0,
            chat_pushed: 0,
            chat_wrap_cache: Default::default(),
            chat_scroll: 0,
            chat_scroll_glide: Default::default(),
            chat_rise: 0.0,
            chat_rise_open: 0.0,
            chat_lines_total: 0,
            chat_bedrock_target: Default::default(),
            chat_bedrock_anim: None,
            chat_bedrock_last_start: 0.0,
            chat_view: Default::default(),
            dimension_types: Vec::new(),
            chat_dropped_lines: 0,
            chat_history_scroll: 0,
            chat_history_glide: Default::default(),
            chat_history_hits: Default::default(),
            chat_link_hits: Default::default(),
            text_selection: None,
            last_left_click: None,
            clipboard: None,
            action_bar: None,
            title_text: None,
            subtitle_text: None,
            title_set_at: Instant::now(),
            title_fade_in: 10,
            title_stay: 70,
            title_fade_out: 20,
            sb_objective_titles: std::collections::HashMap::new(),
            sb_sidebar_objective: None,
            sb_teams: std::collections::HashMap::new(),
            sb_entry_team: std::collections::HashMap::new(),
            tab_entries: std::collections::HashMap::new(),
            tab_header: Vec::new(),
            tab_footer: Vec::new(),
            sb_list_objective: None,
            sb_objective_hearts: std::collections::HashSet::new(),
            sb_objective_formats: std::collections::HashMap::new(),
            sb_score_formats: std::collections::HashMap::new(),
            sb_scores: std::collections::HashMap::new(),
            editing_value: None,
            editing_step: None,
            selected_skins: settings.active_skins.clone(),
            loaded_skin_exclusions: settings.skin_exclusions.iter().filter(|(k, _)| settings.active_skins.contains(k)).map(|(k, v)| (k.clone(), v.clone())).collect(),
            skin_catalog: skin_tab::Catalog::default(),
            skin_drag: None,
            skin_files: None,
            skin_tab_was_open: false,
            system_cursor_visible: std::cell::Cell::new(None),
            modifiers: ModifiersState::empty(),
            tokio_rt: tokio::runtime::Runtime::new().expect("tokio-Runtime konnte nicht erstellt werden"),
            mode: AppMode::Menu,
            // Blickt vom Test-Chunk-Rand (Chunk spannt x/z 0..16, Saeule bis y=12) auf die Struktur.
            camera: render::Camera::new(DVec3::new(8.0, 10.0, -10.0)),
            move_forward: false,
            move_backward: false,
            move_left: false,
            move_right: false,
            jump_held: false,
            sneak_held: false,
            use_held: false,
            use_queue: 0,
            use_placed_block: false,
            use_detonated_anchor: None,
            arrow_empty_at: None,
            use_repeating: false,
            sound_meter: std::cell::Cell::new((0.0, 0.0, None)),
            anchor_wait: None,
            bucket_ack_wait: None,
            use_emptied_bucket: None,
            dimension_type_name: String::new(),
            slot_queue: SlotQueue::default(),
            use_cooldown: 0,
            use_started: false,
            using_item: false,
            using_offhand: false,
            using_item_id: None,
            item_cooldowns: item_cooldowns::ItemCooldowns::default(),
            hotbar_pop: [None; 9],
            breaking_block: false,
            dig_packet_this_tick: false,
            breaking_pos: None,
            breaking_selected: None,
            breaking_progress: 0.0,
            block_breaking_sound_cooldown: 0,
            block_breaking_cooldown: 0,
            attack_broke_block: false,
            game_mode: 0,
            held_name_timer: 0,
            held_name_last: None,
            armor_last_worn: Default::default(),
            camera_entity: None,
            op_level: 0,
            spectator_flight: None,
            kinetic_hit_at: std::collections::HashMap::new(),
            gamemode_wheel: None,
            reconnect_hold: None,
            reconnect_needs_release: false,
            previous_game_mode: None,
            creative: creative::Creative::default(),
            creative_registries: creative::Registries::default(),
            attr_movement_speed: physics::BASE_SPEED,
            attr_attack_speed: None,
            attr_sneaking_speed: physics::DEFAULT_SNEAKING_SPEED,
            attr_water_movement_efficiency: 0.0,
            attr_body: physics::BodyAttributes::default(),
            attr_block_range: BLOCK_INTERACTION_REACH,
            attr_entity_range: ENTITY_INTERACTION_REACH,
            attr_block_break_speed: 1.0,
            attr_mining_efficiency: 0.0,
            attr_submerged_mining_speed: 0.2,
            block_sequence: 0,
            pending_blocks: Vec::new(),
            ghosts_awaiting_mesh: Vec::new(),
            ao_fade_sources: Vec::new(),
            column_light: std::collections::HashMap::new(),
            step_move_dist: 0.0,
            step_next: 1.0,
            ao_fades: Vec::new(),
            food_level: 20,
            food_saturation: 5.0,
            saturation_shown: std::cell::Cell::new(None),
            armor_row_shown: std::cell::Cell::new(None),
            absorption_since: std::cell::RefCell::new(Vec::new()),
            player_names: std::collections::HashMap::new(),
            health: 20.0,
            max_health: 20.0,
            armor: 0,
            xp_level: 0,
            xp_progress: 0.0,
            air_ticks: MAX_AIR_TICKS,
            own_effects: std::collections::HashMap::new(),
            shown_effects: std::collections::HashMap::new(),
            absorption: 0.0,
            ticks_since_last_attack: 100,
            last_melee_hit: None,
            damage_sources: std::collections::HashMap::new(),
            last_attack_hand_item: None,
            attack_hand_changed: false,
            sprint_held: sprint_on_start,
            keybind_physical_down: [false; KeyBindAction::ALL.len()],
            binds_down: Vec::new(),
            bind_presses: Vec::new(),
            keybind_press_order: [0; KeyBindAction::ALL.len()],
            keybind_press_counter: 0,
            keybind_scroll: Default::default(),
            settings_scroll: Default::default(),
            // Vanillas LocalPlayer-Konstruktor bekommt lastSentInput = Input.EMPTY und
            // wasSprinting = false mit. Stuende hier None ("noch nie etwas gesendet"), ginge
            // beim Join ein leeres PlayerInput und ein ueberfluessiges STOP_SPRINTING raus,
            // die Vanilla NIE schickt -- beides im Proxy-Mitschnitt aufgefallen.
            last_sent_player_input: Some((false, false, false, false, false, false, false)),
            pending_input_actions: Vec::new(),
            last_synced_slot: 0,
            own_entity_id: None,
            own_ride: None,
            ride_yaw_spin: (0.0, 0.0),
            boss_bars: Vec::new(),
            own_uuid: None,
            settings_return: MenuScreen::Connect,
            hud_editor_return: MenuScreen::Main,
            notification_history: std::collections::VecDeque::new(),
            last_server_event: Instant::now(),
            transactions: transactions::Transactions::default(),
            packet_gap_note: None,
            lagback_note: None,
            rubberband_note: None,
            notifications_seen: 0,
            notification_panel_open: false,
            notification_panel_since: Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now),
            typed_prev: Vec::new(),
            ui_sound_last: [None; ui_sound::UiSound::ALL.len()],
            ui_sound_level: [1.0; ui_sound::UiSound::ALL.len()],
            erase_pending: false,
            caret_drawn: std::cell::Cell::new(None),
            caret_prev: None,
            falling_glyphs: Vec::new(),
            erase_seed: 0,
            caret_anim: std::cell::Cell::new(None),
            selection_glide: std::cell::RefCell::new(rustc_hash::FxHashMap::default()),
            selection_sound_chars: 0,
            selection_sound_hint: None,
            selection_sound_deselect: false,
            hover_anims: std::cell::RefCell::new(rustc_hash::FxHashMap::default()),
            hover_drawn: std::cell::Cell::new(None),
            chat_slide: std::cell::RefCell::new(None),
            chat_gap: std::cell::RefCell::new(None),
            chat_recall: None,
            app_started: Instant::now(),
            slider_drag: None,
            slider_extended: false,
            step_drag: None,
            step_extended: false,
            step_drag_end: 0.0,
            cam_look: None,
            freelook_held: false,
            freelook_before: None,
            shoulder_side: 1.0,
            shoulder_side_shown: (1.0, Instant::now()),
            aim_until: None,
            own_player_alpha: 1.0,
            settings_search: String::new(),
            search_hits: Vec::new(),
            search_done: None,
            search_flash: None,
            slider_glide: std::cell::RefCell::new(Vec::new()),
            persist_paused: false,
            config_seen: None,
            config_checked: Instant::now(),
            color_popup: None,
            dropdown: None,
            drag_scroll: None,
            drag_scroll_no_click: false,
            section_slide: None,
            settings_press: None,
            press_anim: None,
            section_flash: None,
            server_config_return: None,
            addons: None,
            addon_frame_at: Instant::now(),
            addon_deaths_seen: 0,
            addon_chat_at: None,
            addon_setting_defs: std::collections::HashMap::new(),
            addon_inputs: std::collections::HashMap::new(),
            addon_raw_in: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            addon_hold_packets: false,
            ego_sound_at: None,
            page_scrolls: std::collections::BTreeMap::new(),
            section_click: None,
            scrolls_loaded: false,
            perspective: Perspective::FirstPerson,
            last_sent_sprinting: Some(false),
            last_movement_update: Instant::now(),
            skins: std::collections::HashMap::new(),
            skin_slot_order: std::collections::VecDeque::new(),
            skin_waiters: std::collections::HashMap::new(),
            cape_slot_order: std::collections::VecDeque::new(),
            tab_heads: std::collections::HashMap::new(),
            signs: std::collections::HashMap::new(),
            sign_layouts: Default::default(),
            placed_heads: std::collections::HashMap::new(),
            sign_edit: None,
            book_view: None,
            book_edit: None,
            book_edit_since: Instant::now(),
            book_edit_dragging: false,
            maps: world::map::Maps::default(),
            map_slots: Vec::new(),
            map_frame_counter: 0,
            map_sprites: std::collections::HashMap::new(),
            head_textures: std::collections::HashMap::new(),
            head_lookups: std::collections::HashSet::new(),
            account_skins: std::collections::HashMap::new(),
            skin_pixels: std::collections::HashMap::new(),
            head_profile_cache: std::collections::HashMap::new(),
            account_head_retry: std::collections::HashMap::new(),
            account_switching: None,
            tab_head_order: std::collections::VecDeque::new(),
            requested_skins: std::collections::HashSet::new(),
            skin_sources: std::collections::HashMap::new(),
            skin_retries: std::collections::HashMap::new(),
            own_limbs: world::LimbAnimator::new(),
            own_age_ticks: 0.0,
            own_hurt_ticks: 0,
            own_ticks_since_hurt: u32::MAX,
            // 6000 = Mittag: bis das erste Set Time kommt, ist heller Tag die harmloseste
            // Annahme -- ein Client, der beim Beitritt kurz in tiefer Nacht startet, sieht aus
            // wie ein Fehler.
            world_time_of_day: 6000,
            world_time_ticking: true,
            sky_time_pending: false,
            sky_world_pending: None,
            own_hurt_by_crit: false,
            own_on_fire: false,
            own_swap: world::more_anims::SwapWatch::default(),
            anim_smoothing: Default::default(),
            anim_petting: Default::default(),
            own_last_sword: None,
            own_inspect: None,
            own_inspect_kind: None,
            hand_grip_logged: std::cell::RefCell::new(None),
            hand_debug_item_mode: false,
            hand_debug_string: false,
            hand_debug_bow: None,
            lantern_swing: Default::default(),
            anim_hugging: Default::default(),
            own_frozen_ticks: 0,
            hardcore: false,
            hurt_cam_side: 1.0,
            throw_sweep_slot: None,
            own_potion_swirls: Vec::new(),
            own_potion_swirls_ambient: false,
            own_invisible: false,
            own_glowing: false,
            own_crit_grace: 0,
            own_health_initialized: false,
            own_body_yaw: world::BodyYaw::new(0.0),
            own_use_ticks: 0,
            eat_held_through: false,
            eat_next_use_delay: 0,
            own_render_yaw: 0.0,
            own_render_pitch: 0.0,
            own_last_render_yaw: 0.0,
            own_last_render_pitch: 0.0,
            own_equip_progress: 0.0,
            own_last_equip_progress: 0.0,
            hovered_button: None,
            sounds: audio::bank::SoundBank::new(),
            type_pitch_seed: 1,
            hand_model_wide: world::player_arm_model(false, true),
            hand_model_slim: world::player_arm_model(true, true),
            own_swing_ticks: 0,
            own_cape: world::CapeState::new(),
            module_menu_open: false,
            module_menu_selected: ModuleId::Totem,
            cursor_fx: cursor_fx::CursorFx::new(Instant::now()),
            notifications: notifications::Notifications::new(),
            wheel_accum: 0.0,
            volume_shown_at: None,
            volume_entered_at: None,
            volume_key_repeat: None,
            volume_unsaved: false,
            pack_load_job: None,
            pack_vorrat: Vec::new(),
            pack_aktiv: None,
            skin_zips: skin_zips::SkinZips::default(),
            server_packs: server_packs::ServerPacks::default(),
            shield_color_editor: None,
            shield_cooldowns: rustc_hash::FxHashMap::default(),
            trails: rustc_hash::FxHashMap::default(),
            trail_tick: 0,
            module_menu_scroll: Default::default(),
            module_menu_category: ModuleCategory::CrystalPvp,
            module_menu_detail: false,
            swallow_text_key: None,
            totem_tracker: modules::TotemTracker::new(),
            totem_animation: None,
            floating_totem: None,
            player_loaded_pending: false,
            player_loaded_have_pos: false,
            client_loaded: false,
            load_tracker_ticks: 0,
            initial_chunks_coming: false,
            world_loading_since: Instant::now(),
            column_meshed_at: rustc_hash::FxHashMap::default(),
            lod: lod::LodState::default(),
            world_height: 384,
            totem_hand_published: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(modules::TotemHand::OffHand.to_published())),
            own_pop_plan: std::sync::Arc::new(std::sync::Mutex::new(toteming::OwnPopPlan::Silent)),
            own_pop_plan_key: None,
            custom_pop_sounds: toteming::CustomSounds::default(),
            pop_counts: std::collections::HashMap::new(),
            crystal_opted_out: false,
            crystal_opt_out_servers: Vec::new(),
            consumable: consumable_optimizer::State::default(),
            crystal_opt_out_notified: Vec::new(),
            crystal_opt_out_notice_at: None,
            // -1 kann keine echte EntityId sein -- solange der Login nicht durch ist, trifft
            // der Haken also auf nichts.
            own_entity_id_published: std::sync::Arc::new(std::sync::atomic::AtomicI32::new(-1)),
            own_leaning_pitch: 0.0,
            own_last_leaning_pitch: 0.0,
            own_gliding_ticks: 0,
            own_tick_vel: DVec3::ZERO,
            own_last_tick_vel: DVec3::ZERO,
            physics: physics::PlayerPhysics::new(DVec3::new(8.0, 10.0, -10.0)),
            physics_accum: 0.0,
            verworfene_tickzeit: 0.0,
            letzte_tickverlust_meldung: Instant::now(),
            physics_render_prev: DVec3::new(8.0, 10.0, -10.0),
            pistons: pistons::Pistons::default(),
            // Startwert ist die STEHENDE Augenhoehe, nicht Vanillas 0.0. Vanillas Camera wird
            // einmal beim Spielstart gebaut und ist laengst eingeschwungen, bevor man eine Welt
            // betritt; mit 0.0 wuerde die Kamera hier bei jedem Weltbeitritt sichtbar von den
            // Fuessen nach oben kriechen -- ein Artefakt, das es in Vanilla nicht zu sehen gibt.
            camera_eye_height: physics::EYE_HEIGHT,
            camera_eye_height_prev: physics::EYE_HEIGHT,
            last_sent_pos: DVec3::new(8.0, 10.0, -10.0),
            last_sent_yaw_deg: 0.0,
            last_sent_pitch_deg: 0.0,
            last_sent_on_ground: false,
            last_sent_horizontal_collision: false,
            ticks_since_position_sent: 0,
            cursor_pos: (960.0, 540.0),
            focused: true,
            screen: MenuScreen::Connect,
            hud_drag_offset: None,
            font_drag: None,
            hud_selected: None,
            crosshair_painter: None,
            is_dead: false,
            sleeping_at: None,
            settings,
            settings_status: None,
            audio,
            pipewire: pipewire::Status::default(),
            audio_devices,
            orbit_test: None,
            tracked_sounds: Vec::new(),
            wind_charge_shift: None,
            wind_charge_pixels: None,
            jukeboxes: std::collections::HashMap::new(),
            jukebox_songs: Vec::new(),
            cat_variants: Vec::new(),
            anvil_name: String::new(),
            anvil_name_for: None,
            discord: discord_rpc::DiscordPresence::start(),
            discord_start: discord_rpc::unix_now(),
            last_orbit_update: Instant::now(),
            awaiting_keybind: None,
            sens_last_changed_y: false,
            keybind_advanced: None,
            tearfree_state: None,
            tearfree_original_value: None,
            tearfree_user_choice: None,
            debug_overlay: false,
            hud_hidden: false,
            zoom_held: false,
            push_to_talk_held: false,
            voice: voicechat::VoiceChat::default(),
            inspect_held: false,
            whisper_held: false,
            zoom_amount: 0.0,
            zoom_updated: None,
            profiler: profiler::FrameProfiler::new(),
            frame_pool: crate::parallel::FramePool::with_default_size(),
            cull_frustum: std::cell::Cell::new(None),
            skin_frame: std::cell::RefCell::default(),
            skin_cache: std::cell::RefCell::default(),
            vblank: None,
            vblank_tried: false,
            tear_slot_ns: None,
            tear_present_at: None,
            tear_lead_ns: 3_000_000.0,
            sys_stats: sysstats::SystemStats::new(),
            memory_info: std::cell::Cell::new(None),
            compositing_info: std::sync::Arc::new(std::sync::Mutex::new(None)),
            compositing_checked: std::cell::Cell::new(None),
            window_path: None,
            window_path_tried: false,
            log_lines: Vec::new(),
            conn_status: ConnStatus::Disconnected,
            conn_route: None,
            events_tx,
            events_rx,
            tabs: vec![app::ServerTab::new()],
            active_tab: 0,
            event_backlog: std::collections::VecDeque::new(),
            replaying_backlog: false,
            outgoing_tx: None,
            chat_signer: None,
            shared_rotation: std::sync::Arc::new(net::SharedLook::default()),
            conn_task: None,
            chunks_received: 0,
            column_history: std::collections::HashMap::new(),
            world_reset_debug: None,
            world: world::World::new(),
            entities: world::Entities::new(),
            mesh_results_tx,
            mesh_results_rx,
            mesh_pool,
            render_limit_meshed: rustc_hash::FxHashSet::default(),
            render_limit_order: Vec::new(),
            render_limit_order_key: None,
            mesh_jobs_pending: std::collections::HashSet::new(),
            mesh_jobs_stale_on_completion: std::collections::HashSet::new(),
            camera_placed: false,
        }
    }

    // Den Skin-Ordner im Dateimanager zeigen. Er wird dabei ANGELEGT, falls es ihn noch
    // nicht gibt: beim ersten Mal existiert er nie, und ein Knopf, der dann nichts tut,
    // waere schlimmer als keiner.
    // Configs-Seite "Ordner öffnen": der Ordner mit den weitergebbaren Configs.
    pub(crate) fn open_configs_dir(&mut self) {
        let Some(dir) = assets::configs_dir() else {
            self.push_log("Config-Ordner unbekannt (kein HOME gesetzt)".to_string());
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.push_log(format!("Config-Ordner konnte nicht angelegt werden ({e}): {}", dir.display()));
            return;
        }
        match open_external_command(&dir).spawn() {
            Ok(_) => self.push_log(format!("Config-Ordner geöffnet: {}", dir.display())),
            Err(e) => self.push_log(format!("Config-Ordner konnte nicht geöffnet werden ({e}): {}", dir.display())),
        }
    }

    pub(crate) fn open_skins_dir(&mut self) {
        let Some(dir) = skins_dir() else {
            self.push_log("Skin-Ordner unbekannt (kein HOME gesetzt)".to_string());
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.push_log(format!("Skin-Ordner konnte nicht angelegt werden ({e}): {}", dir.display()));
            return;
        }
        match open_external_command(&dir).spawn() {
            Ok(_) => self.push_log(format!("Skin-Ordner geöffnet: {}", dir.display())),
            Err(e) => self.push_log(format!("Skin-Ordner konnte nicht geöffnet werden ({e}): {}", dir.display())),
        }
    }

    // Skin-Tab "Laden" (s. ButtonAction::LoadSkin): baut Atlas/Colormaps/block_uvs/Formen
    // aus self.installation mit dem NEUEN Resourcepack-Ordner neu (dieselbe Bau-Logik wie
    // App::new, s. WorldAssets-Struct-Kommentar) und tauscht sie live aus -- funktioniert
    // AUCH waehrend einer laufenden Verbindung (anders als der fruehere reine Settings-Pfad,
    // der erst beim naechsten Programmstart wirkte). `skins`: Ordnernamen unter skins_dir(), oberstes Pack zuerst,
    // leer fuer "Kein Pack" (reines Vanilla-Verhalten).
    //
    // UV-Bruchteile bereits gemeshter Chunks bleiben i.A. gueltig (s. rebuild_atlas-
    // Kommentar im render-Crate) -- trotzdem wird sicherheitshalber ALLES neu gemesht: ein
    // Pack koennte theoretisch auch Modelle/Blockstates mit-ueberschreiben (aendert
    // Formen/Rotationen, die WorldAssets::build ebenfalls neu baut) und wuerde sonst nur
    // beim naechsten echten Blockupdate sichtbar.
    //
    // Der Bau selbst dauert Sekunden und lief frueher im Hauptthread -- alles fror so lange
    // ein. Jetzt baut ein eigener Thread mit einer eigenen Installation alles, was
    // keine GPU braucht, und meldet den Fortschritt; eine Benachrichtigung zeigt ihn an.
    // Ausgetauscht wird erst, wenn alles fertig ist (s. poll_resource_pack_job).
    pub(crate) fn reload_resource_pack(&mut self, skins: Vec<String>) {
        if self.pack_load_job.is_some() {
            self.notify(notifications::Notification::warning("Skin wird schon geladen", "Bitte warten, bis der laufende Wechsel fertig ist."));
            return;
        }
        self.server_packs.begin_reload();
        let PackPlan { skins, exclusions, server_ids, oben, unten, key } = self.pack_plan(skins);
        let server = self.server_packs.ready();
        let platz = (!server.is_empty()).then(|| self.server_pack_position(&server));
        skin_tab::normalize_stack(&mut self.selected_skins, platz);
        // Fuer diesen Stapel schon vorgebaut (anderer Tab): das nehmen statt neu zu bauen.
        let rx = match self.pack_vorrat.iter().position(|(k, _)| *k == key).map(|i| self.pack_vorrat.remove(i).1) {
            Some(PackVorrat::Fertig(pack)) => {
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = tx.send(PackLoadMsg::Done(Ok(pack)));
                rx
            }
            Some(PackVorrat::Laeuft(rx)) => rx,
            None => match spawn_pack_build(oben, unten, false) {
                Ok(rx) => rx,
                Err(e) => {
                    self.push_log(format!("Skin konnte nicht geladen werden: {e}"));
                    return;
                }
            },
        };
        self.pack_aktiv = Some(key);
        let (titel, text) = pack_load_note_text(&skins, !server_ids.is_empty(), 0);
        // Steht, bis der Wechsel fertig ist -- dann wird sie geschlossen (s. poll_resource_pack_job).
        // Kommt das Pack aus dem Vorrat, ist sie schon im naechsten Bild wieder weg.
        let note = self.notify(notifications::Notification::info(titel, text).with_lifetime(Duration::from_hours(1)));
        self.pack_load_job = Some(PackLoadJob { rx, note, skins, exclusions, server_ids });
    }

    // Der Stapel, den reload_resource_pack fuer `skins` und die fertigen Server-Packs baut --
    // ohne etwas umzustellen, damit er sich auch fuer einen anderen Tab ausrechnen laesst (s.
    // fill_pack_vorrat).
    pub(crate) fn pack_plan(&self, skins: Vec<String>) -> PackPlan {
        // Server-Resourcepacks kommen neu an ihren gemerkten Platz im Stapel, sonst ganz oben wie
        // bei Vanilla; im Skin-Tab lassen sie sich danach beliebig verschieben.
        let server = self.server_packs.ready();
        let server_ids: Vec<[u8; 16]> = server.iter().map(|(id, _)| *id).collect();
        let platz = (!server.is_empty()).then(|| self.server_pack_position(&server));
        let mut skins = skins;
        skin_tab::normalize_stack(&mut skins, platz);
        let exclusions = self.skin_exclusions_of(&skins);
        let server_pfade: Vec<(std::path::PathBuf, Vec<String>)> = server
            .into_iter()
            .map(|(id, path)| {
                let aus = self.server_pack_exclusions(id, &path);
                (path, aus)
            })
            .collect();
        let (oben, unten) = skin_stack_dirs(&skins, &exclusions, &server_pfade);
        let key = pack_stack_key(&oben, &unten);
        PackPlan { skins, exclusions, server_ids, oben, unten, key }
    }

    // Mehrere Tabs: die Packs der anderen Tabs im Hintergrund vorbauen, damit ein Wechsel sie nur
    // noch einsetzt. Was kein Tab mehr braucht, faellt weg.
    pub(crate) fn fill_pack_vorrat(&mut self) {
        let mut gebraucht = Vec::new();
        for j in 0..self.tabs.len() {
            if j == self.active_tab {
                continue;
            }
            std::mem::swap(&mut self.server_packs, &mut self.tabs[j].server_packs);
            let plan = self.pack_plan(self.settings.active_skins.clone());
            std::mem::swap(&mut self.server_packs, &mut self.tabs[j].server_packs);
            if self.pack_aktiv.as_ref() != Some(&plan.key) && !gebraucht.contains(&plan.key) {
                gebraucht.push(plan.key.clone());
                if !self.pack_vorrat.iter().any(|(k, _)| *k == plan.key) {
                    match spawn_pack_build(plan.oben, plan.unten, true) {
                        Ok(rx) => self.pack_vorrat.push((plan.key, PackVorrat::Laeuft(rx))),
                        Err(e) => self.push_log(format!("Pack fuer den anderen Tab nicht vorgebaut: {e}")),
                    }
                }
            }
        }
        self.pack_vorrat.retain(|(k, _)| gebraucht.contains(k));
    }

    // Vorgebaute Packs abholen, sobald ihr Thread fertig ist.
    pub(crate) fn poll_pack_vorrat(&mut self) {
        for (key, eintrag) in &mut self.pack_vorrat {
            let PackVorrat::Laeuft(rx) = eintrag else { continue };
            loop {
                match rx.try_recv() {
                    Ok(PackLoadMsg::Progress(_)) => {}
                    Ok(PackLoadMsg::Done(Ok(pack))) => {
                        *eintrag = PackVorrat::Fertig(pack);
                        break;
                    }
                    Ok(PackLoadMsg::Done(Err(e))) => {
                        eprintln!("[pack] Vorrat {key}: {e}");
                        break;
                    }
                    Err(_) => break,
                }
            }
        }
    }

    // Einmal je Bild: Meldungen des Pack-Threads abholen, den Fortschritt anzeigen und am
    // Ende das fertige Pack einsetzen.
    pub(crate) fn poll_resource_pack_job(&mut self) {
        self.poll_pack_vorrat();
        // Server-Packs fertig geladen (oder weggefallen): neu zusammenstellen, sobald nichts
        // anderes laedt.
        if self.pack_load_job.is_none() && self.server_packs.wants_reload() {
            self.reload_resource_pack(self.settings.active_skins.clone());
        }
        let Some(job) = &self.pack_load_job else { return };
        let mut fertig = None;
        let mut prozent = None;
        while let Ok(msg) = job.rx.try_recv() {
            match msg {
                PackLoadMsg::Progress(p) => prozent = Some(p),
                PackLoadMsg::Done(r) => {
                    fertig = Some(r);
                    break;
                }
            }
        }
        // Thread weg ohne Ergebnis (Panik) -- wie ein Fehler behandeln.
        let abgerissen = fertig.is_none() && matches!(job.rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected));
        let (note, name, server_ids, exclusions) = (job.note, job.skins.clone(), job.server_ids.clone(), job.exclusions.clone());
        if let Some(p) = prozent {
            let (titel, text) = pack_load_note_text(&name, !server_ids.is_empty(), p);
            self.notifications.set_text(note, titel, text);
        }
        if abgerissen {
            fertig = Some(Err("Ladevorgang abgebrochen".to_string()));
        }
        let Some(ergebnis) = fertig else { return };
        self.pack_load_job = None;
        self.notifications.dismiss(note, Instant::now());
        let status = if ergebnis.is_ok() { protocol::packets::ResourcePackStatus::SuccessfullyLoaded } else { protocol::packets::ResourcePackStatus::FailedReload };
        for id in self.server_packs.end_reload(&server_ids, ergebnis.is_ok()) {
            self.send_pack_status(id, status);
        }
        match ergebnis {
            Ok(pack) => {
                let titel = if !server_ids.is_empty() {
                    "Server-Resourcepack geladen".to_string()
                } else if name.is_empty() {
                    "Vanilla-Texturen geladen".to_string()
                } else {
                    format!("Skin geladen: {}", skin_stack_label(&name))
                };
                self.apply_resource_pack(*pack, name);
                self.loaded_skin_exclusions = exclusions;
                if self.tabs.len() > 1 {
                    self.fill_pack_vorrat();
                }
                self.notify(notifications::Notification::success(
                    titel,
                    "",
                ));
            }
            Err(e) => {
                self.push_log(format!("Skin konnte nicht geladen werden: {e}"));
                self.notify(notifications::Notification::error("Skin konnte nicht geladen werden", e));
            }
        }
    }

    // Das im Hintergrund gebaute Pack einsetzen -- der Teil, der GPU und App-Zustand braucht.
    pub(crate) fn apply_resource_pack(&mut self, pack: LoadedPack, skins: Vec<String>) {
        let LoadedPack { installation, sky, assets, sounds, glint, weather, fonts } = pack;
        self.installation = installation;
        // Das Bild fuer Server ohne Icon kann aus einem Pack kommen -- neu holen, falls schon
        // geladen.
        if std::mem::take(&mut self.unknown_server_icon) {
            self.upload_unknown_server_icon();
        }
        if let Some(gpu) = self.gpu.as_mut() {
            for id in self.custom_sky.gpu_textures() {
                gpu.custom_sky.remove_texture(id);
            }
        }
        self.custom_sky = custom_sky::CustomSky::new(sky);
        if !self.custom_sky.is_empty() {
            self.push_log(format!("Himmel aus dem Pack: {} Ebenen", self.custom_sky.layer_count()));
        }
        // Ein Pack darf eigene Sprachdateien mitbringen.
        apply_language_settings(&mut self.installation, &self.settings);
        let WorldAssets {
            atlas,
            ui_atlas,
            item_icons,
            entity_atlas,
            particle_atlas,
            entity_models,
            player_wide,
            player_slim,
            cape_model,
            alt_font,
            bed_head_model,
            bed_foot_model,
            armor_models,
            shield,
            grass_colormap,
            foliage_colormap,
            dry_foliage_colormap,
            block_uvs,
            block_shapes,
            cross_blocks,
            cross_extra_shapes,
        } = assets;
        block_uvs.set_rng_rotation_enabled(self.settings.block_rng_rotation);

        self.particles.set_textures(&particle_atlas, atlas.particle_uvs.iter().map(|(k, v)| (k.clone(), *v)).collect());
        self.particle_atlas = particle_atlas;
        self.atlas_mips = atlas.mips;
        self.atlas_size = atlas.size;
        self.ui_atlas = ui_atlas;
        self.ui_anim_last.clear();
        self.item_icons = ItemIcons::new(item_icons);
        self.entity_atlas = entity_atlas;
        set_shield_icon(&mut self.item_icons, &self.shield, &self.entity_atlas);
        set_special_icons(&mut self.item_icons, &self.entity_atlas, &mut self.installation);
        self.entity_models = entity_models;
        self.player_wide = player_wide;
        self.player_slim = player_slim;
        self.cape_model = cape_model;
        self.alt_font = alt_font;
        self.bed_head_model = bed_head_model;
        self.bed_foot_model = bed_foot_model;
        self.armor_models = armor_models;
        self.shield = shield;
        self.water_animation_frames = atlas.water_animation_frames;
        self.water_tile_origin = atlas.water_tile_origin;
        self.water_flow_animation_frames = atlas.water_flow_animation_frames;
        self.water_flow_tile_origin = atlas.water_flow_tile_origin;
        self.water_tile_size = atlas.tile_size;
        self.water_anim_frame = 0;
        self.water_anim_last_switch = Instant::now();
        self.block_animations = block_anim::BlockAnimations::new(atlas.animations);
        self.block_uvs = block_uvs;
        self.block_shapes = block_shapes;
        // Die Voxel-Klassen des Shader-Pfads haengen an den Formen: beim naechsten Frame neu.
        #[cfg(feature = "shader_path")]
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.shader_path = None;
        }
        self.cross_blocks = cross_blocks;
        self.cross_extra_shapes = cross_extra_shapes;
        self.grass_colormap = grass_colormap;
        self.foliage_colormap = foliage_colormap;
        self.dry_foliage_colormap = dry_foliage_colormap;

        // Der Ton haengt genauso am Pack wie die Texturen: ein Pack darf eigene sounds.json
        // und eigene .ogg-Dateien mitbringen. Ohne diesen Neuaufbau bliebe nach einem
        // Pack-Wechsel der Klang-Index des ALTEN Packs stehen -- und die schon dekodierten
        // Aufnahmen erst recht, die liegen ja im Zwischenspeicher.
        self.vanilla_sounds = sounds;
        // Der Pop-Plan haelt Aufnahmen des alten Packs -- beim naechsten Tick neu aufloesen.
        self.own_pop_plan_key = None;

        if let Some(gpu) = self.gpu.as_mut() {
            gpu.block_renderer.rebuild_atlas(&gpu.device, &gpu.queue, &self.atlas_mips, self.atlas_size);
            let ziel = block_anim::UploadTarget {
                device: gpu.device.clone(),
                queue: gpu.queue.clone(),
                texture: gpu.block_renderer.atlas_texture_shared(),
            };
            self.block_animations.set_target(ziel);
            // Beide Sprite-Bind-Groups zeigen jetzt auf veraltete Texturen (Blockatlas neu
            // hochgeladen, UI-Atlas neu gebaut) -- sonst blieben Hotbar und Item-Icons auf
            // dem alten Pack stehen.
            gpu.rebuild_sprite_atlases(&self.ui_atlas.rgba, self.ui_atlas.size);
            // Und die ENTITY-Textur ebenso: dort liegen Skins, Mobs, Betten, Ruestung und das
            // Schild. Sie blieb bisher als einzige unangetastet -- ein Pack mit eigener
            // Diamantruestung zeigte bis zum Neustart weiter die Vanilla-Textur.
            gpu.block_renderer.rebuild_entity_atlas(&gpu.queue, &self.entity_atlas.rgba, self.entity_atlas.size);
            self.wind_charge_shift = None;
            // Die Tab-Koepfe liegen als Kopie vor und kommen sofort zurueck.
            for (slot, pixel) in self.tab_heads.values() {
                if let Some(origin) = self.entity_atlas.head_slot_origin(*slot) {
                    gpu.block_renderer.upload_item(&gpu.queue, origin, assets::HEAD_SLOT_WIDTH, assets::HEAD_SLOT_HEIGHT, pixel);
                }
            }
            // Die Glint-Texturen kann ein Pack ebenfalls ersetzen.
            let glint_texturen = glint;
            set_glint_textures(&mut gpu.block_renderer, &gpu.device, &gpu.queue, &glint_texturen);
            gpu.set_gui_glint_texture(&glint_texturen[0]);
            self.weather_textures = weather;
            self.weather_textures_uploaded = None;
            gpu.block_renderer.rebuild_particle_atlas(&gpu.device, &gpu.queue, &self.particle_atlas.rgba, self.particle_atlas.size);
            gpu.block_renderer.clear_meshes();
            gpu.set_pack_fonts(fonts);
        }
        self.release_atlas_pixels();
        self.render_limit_meshed.clear();
        self.lod.mask_changed = true;
        self.column_meshed_at.clear();
        // Items in Haenden zeigen auf Plaetze im ALTEN Entity-Atlas und auf Modelldaten des
        // alten Packs -- beides ist jetzt hinfaellig. Ohne das behielte ein Schwert die
        // Textur des vorherigen Packs, bis es einmal aus der Sicht verschwindet.
        self.held_items.clear();
        self.map_slots.clear();
        self.trim_textures.clear();
        self.trim_slot_order.clear();
        self.equipment_models.clear();
        // Skins und Umhaenge lagen in den reservierten Plaetzen der Entity-Textur, die eben
        // vollstaendig ueberschrieben wurde -- ihre Pixel sind weg. Die Platzvergabe deshalb
        // zuruecksetzen und die laufende Anfragesperre loesen: refresh_missing_skins holt sie
        // pro Frame ueber `skin_sources` von selbst wieder herein, genau wie nach einer
        // Verdraengung (s. dort). Ohne das stuenden alle Spieler dauerhaft weiss da.
        for eintrag in self.skins.values_mut() {
            eintrag.slot = None;
            eintrag.cape_slot = None;
        }
        self.requested_skins.clear();
        // Die Koepfe der Konten oben rechts gleich wieder hinein -- beim Start wird das erste
        // Pack oft erst NACH ihnen fertig, und bis zum Oeffnen der Kontenliste stuende sonst
        // der Ersatz da.
        self.request_account_heads();
        for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
            self.world.mark_dirty(cx, cz);
        }

        self.push_log(if skin_stack_has_packs(&skins) {
            format!("Skin geladen: {}", skin_stack_label(&skins))
        } else {
            "Skin entfernt (Vanilla-Texturen)".to_string()
        });
        self.settings.active_skins = skins;
        self.persist_settings();
        // Ein Pack kann Core-Shader ersetzen (oder sie mit dem Wechsel wieder hergeben).
        self.apply_pack_shaders();
    }
}

// --- Auswahl der Statusleisten-Symbole -------------------------------------------------------
//
// Alle drei Leisten zaehlen in HALBEN Punkten: Symbol `index` deckt die Punkte 2*index+1
// (halb) und 2*index+2 (voll) ab. Als eigene Funktionen, weil genau hier die
// Ab-eins-daneben-Fehler sitzen -- ein halbes Herz zu frueh oder zu spaet faellt im Spiel
// kaum auf, ist aber falsch.
