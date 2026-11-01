// winit-Fenster, Input-Handling, Main-Loop, verbindet alle Crates.
//
// Menu-Modus: Server-Adresseingabe, Connect-/Tearing-Test-Button, Log-Panel mit
// Verbindungsstatus. World-Modus (Tab): freistehender Test-Chunk mit Fly-Kamera.

// Unter Windows kein Konsolenfenster neben dem Spiel. Die Ausgabe landet trotzdem im Log, weil
// der Launcher stdout/stderr in client.log umleitet.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod addons;
mod block_anim;
mod block_place;
mod book;
mod hero_scene;
mod chat_input;
mod text_field;
mod caps_lock;
mod chat_log;
mod consumable_optimizer;
mod creative;
mod creative_screen;
mod dialog;
mod dialog_screen;
mod crosshair_editor;
mod crystal_optimizer;
mod cursor_fx;
mod transactions;
mod discord_rpc;
mod font_atlas;
mod gpu_timer;
mod held_item;
mod hitbox_kinds;
mod teams;
mod i18n;
mod inventory;
mod item_cooldowns;
mod item_icons;
mod item_model_input;
mod latency_tuning;
mod loading_screen;
mod link_whitelist;
mod lod;
mod mesh_pool;
mod particles;
mod input_log;
mod modules;
mod monitor_size;
mod notifications;
mod physics;
mod pistons;
mod profiler;
mod raw_input;
mod recipe_book;
mod server_packs;
mod skin_tab;
mod skin_zips;
mod screenshot;
mod custom_sky;
mod precipitation;
mod sound_log;
mod spatial_sound;
mod spectator;
mod gamemode_wheel;
mod stable_numbers;
mod server_status;
mod theme;
mod toteming;
mod sysstats;
mod pipewire;
mod system_info;
mod ui_sound;
mod vblank;
mod vehicle;
mod wallpaper;
mod last_view;
mod armor_hud;
mod keystrokes;
mod session_info;
mod smooth_scroll;
mod death_burst;
mod ego_name;
mod vanilla_sounds;
mod voicechat;
mod app;
mod labels;
mod item_text;
mod text_markup;
mod ui_layout;
mod styled_text;
mod settings_layout;
mod game_misc;
mod ui_anim;
mod ui_geometry;
mod crosshair_shapes;
mod visual_options;
mod crosshair_verts;
mod text_draw;
mod gpu_state;
mod value_format;
mod menu_state;
mod keybinds;
mod settings;
mod server_list;
mod extras;
mod settings_io;
mod settings_defaults;
mod actions;
mod module_ids;
mod widgets;
mod app_state;
mod world_assets;
mod effects_hud;
pub(crate) use labels::*;
pub(crate) use item_text::*;
pub(crate) use text_markup::*;
pub(crate) use ui_layout::*;
pub(crate) use styled_text::*;
pub(crate) use settings_layout::*;
pub(crate) use game_misc::*;
pub(crate) use ui_anim::*;
pub(crate) use ui_geometry::*;
pub(crate) use crosshair_shapes::*;
pub(crate) use visual_options::*;
pub(crate) use crosshair_verts::*;
pub(crate) use text_draw::*;
pub(crate) use gpu_state::*;
pub(crate) use value_format::*;
pub(crate) use menu_state::*;
pub(crate) use keybinds::*;
pub(crate) use settings::*;
pub(crate) use server_list::*;
pub(crate) use extras::*;
pub(crate) use settings_io::*;
pub(crate) use actions::*;
pub(crate) use module_ids::*;
pub(crate) use widgets::*;
pub(crate) use app_state::*;
pub(crate) use world_assets::*;
pub(crate) use effects_hud::*;

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use font_atlas::{FontAtlas, FontKind};
use glam::{DVec3, Vec3};
use inventory::Inventory;
use item_icons::{ItemIcons, SpriteAtlas, UiSprite};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

const MAX_ADDRESS_LEN: usize = 64;
// Wie viele Nachrichten der Verlauf haelt -- durchblaettern laesst er sich bei offenem Chat
// mit dem Mausrad (Vanilla: 100).
const CHAT_MAX_MESSAGES: usize = 1000;
// Hoechstens so viele Zeilen pro Bild, wenn man beim Markieren aus dem Chat herauszieht
// (s. chat_selection_autoscroll). Bei 60 Bildern/s sind das gut zwei Bildschirme pro
// Sekunde -- schnell genug zum Durchblaettern, langsam genug zum Zielen.
const CHAT_AUTOSCROLL_MAX_LINES: f32 = 3.0;

fn create_game_window(event_loop: &ActiveEventLoop) -> (Arc<Window>, winit::monitor::VideoModeHandle) {
    let monitor = event_loop.primary_monitor().expect("kein Monitor gefunden");
    let video_mode = monitor
        .video_modes()
        .max_by_key(|m| u64::from(m.size().width) * u64::from(m.size().height) * u64::from(m.refresh_rate_millihertz()))
        .expect("kein Video-Mode verfügbar");

    // Exclusive statt Borderless: probeweise getauscht, um zu pruefen, ob der
    // XRandR-Modus-Wechsel das Tearing bei Fifo verursacht -- tat er nicht, beide
    // Modi reissen mit unseren eigenen (verlaesslichen) Tearing-Balken identisch.
    // Zurueckgesetzt auf Exclusive (bewusste Wahl von vorher, unabhaengig davon).
    // Bleibt auch unter Windows Exclusive -- Borderless kostet genau die Latenz, um
    // die es dem Projekt geht. Das Alt-Tab/Win+D-Problem dort wird stattdessen bei
    // Fokusverlust geloest (s. WindowEvent::Focused: set_minimized unter Windows).
    let window_attrs = Window::default_attributes()
        .with_title("Coral Client")
        .with_fullscreen(Some(Fullscreen::Exclusive(video_mode.clone())));
    // Stabile WM_CLASS statt dem vom Fenstermanager erratenen Standardwert --
    // scripts/tearfree-auto.sh erkennt unser Fenster daran zuverlaessig. Nur X11.
    #[cfg(target_os = "linux")]
    let window_attrs = {
        use winit::platform::x11::WindowAttributesExtX11;
        window_attrs.with_name("coral-client", "coral-client")
    };

    let window = event_loop.create_window(window_attrs).expect("Fenster konnte nicht erstellt werden");
    // KEIN eigener _NET_WM_BYPASS_COMPOSITOR-Hint mehr: empirisch bestaetigt, dass er
    // mit Muffins eigener "Komposit fuer Vollbildfenster deaktivieren"-Heuristik
    // kollidiert und einen echten Hardware-Flip verhindert, statt ihn zu erzwingen --
    // mit Hint riss Fifo trotz exakt getakteter 120 FPS, ohne Hint (Muffins Heuristik
    // allein entscheidet) nicht mehr. TearFree brauchte es dadurch ueberhaupt erst,
    // was wiederum eine zusaetzliche Scanout-Pufferstufe (und damit Latenz) kostet.
    let window = Arc::new(window);
    (window, video_mode)
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }

        let (window, video_mode) = self.startup_window.take().unwrap_or_else(|| create_game_window(event_loop));
        let breite = video_mode.size().width;
        if let Some(mm) = monitor_size::width_mm(breite) {
            PX_PER_MM.store((breite as f32 / mm).to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
        let refresh_mhz = video_mode.refresh_rate_millihertz();
        if refresh_mhz > 0 {
            self.monitor_frame_interval = Duration::from_secs_f64(1000.0 / f64::from(refresh_mhz));
        }

        let mut gpu = GpuState::new(
            window,
            &mut self.installation,
            &self.atlas_mips,
            self.atlas_size,
            &self.entity_atlas.rgba,
            self.entity_atlas.size,
            &self.particle_atlas,
            &self.ui_atlas.rgba,
            self.ui_atlas.size,
            FontKind::from_index(self.settings.font),
        );
        self.cursor_pos = (gpu.size().0 / 2.0, gpu.size().1 / 2.0);
        gpu.stable_numbers.borrow_mut().set_enabled(self.settings.stable_numbers);
        font_atlas::set_capitals(self.settings.font_capitals);
        gpu.upload_logos();
        self.gpu = Some(gpu);
        self.sync_text_shadow();
        self.apply_pack_shaders();
        self.connect_block_animations();
        self.request_wallpaper(true);
        // Kopf oben in der Leiste (und der Liste) gleich zeigen, nicht erst beim Oeffnen der
        // Kontenliste -- sonst steht bis dahin der Standard-Skin da.
        self.request_account_heads();
        // Erst jetzt moeglich: der Treiber meldet seine Present-Modi erst mit der Surface.
        self.apply_frame_latency();
        // Nur einmal abfragen, nicht bei jedem Frame -- jeder Aufruf startet einen
        // xrandr-Subprozess. Fuer JEDE GPU: ob es TearFree gibt, entscheidet der Treiber
        // (s. latency_tuning.rs). Ohne die Eigenschaft bleibt es bei None (Schalter "n/v").
        // Schnappschuss des ROHWERTS vor jeder eigenen Aenderung -- auf genau den wird bei
        // Fokusverlust/Beenden zurueckgestellt.
        self.tearfree_original_value = latency_tuning::tearfree_value();
        self.tearfree_state = self.tearfree_original_value.as_deref().map(|v| v == "on");
        // Erst jetzt moeglich: owns_mouse() braucht das Fenster (Fullscreen-Status).
        self.refresh_cursor_visibility();
        self.push_log("Coral Client bereit.");
        self.next_frame_time = Instant::now();
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame_time));
    }

    // Deckt jeden Ausstiegsweg ab (Fenster zu, Alt+F4, Strg+C) an einer einzigen Stelle,
    // statt an jedem event_loop.exit()-Aufruf einzeln daran zu denken.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Zuerst das Fenster weg (auch aus Taskleiste und Alt-Tab): das Aufraeumen danach
        // (TearFree, Fernsicht speichern, GPU freigeben) soll man nicht als haengendes Fenster sehen.
        if let Some(gpu) = self.gpu.as_ref() {
            gpu.window.set_visible(false);
        }
        self.restore_original_tearfree();
        // Chunk-Saver: Geaendertes noch speichern, dann schreibt er beim Beenden alles weg.
        self.lod_save_dirty();
        self.lod_shutdown();
        // coral-record: offene Aufnahmen mit Index abschliessen. Was die Verbindungen noch
        // puffern (hoechstens 200 ms), fehlt dann.
        self.recorder.flush();
        record::finish(Duration::from_millis(500));
    }

    // Nur fuer den Frame-Profiler: wie spaet das System die Schleife nach einem WaitUntil
    // tatsaechlich weckt (Timer-Aufloesung, Scheduler).
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let Some(since) = self.loop_idle_since.take() {
            self.frame_stats.sleep(since.elapsed().as_secs_f32() * 1000.0);
        }
        if let StartCause::ResumeTimeReached { start, requested_resume } = cause {
            self.profiler.wake(requested_resume, start);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.loop_step(event_loop);
        // Was waehrend des Durchlaufs fuer Hook::PacketOut zurueckgehalten wurde, jetzt raus.
        self.flush_addon_packets();
        // Bis new_events schlaeft die Schleife -- keine Arbeit (s. FrameStats).
        self.loop_idle_since = Some(Instant::now());
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, event: DeviceEvent) {
        if self.gpu.is_none() {
            return;
        }
        // winits MouseMotion (Kernel-HID -> libinput -> XInput2 -> winit) NUR verwenden,
        // wenn der evdev-Raw-Bypass nicht laeuft. Sonst kaeme dieselbe Bewegung doppelt an
        // (einmal roh in about_to_wait, einmal hier) -- und der laggy X11-Weg wuerde die
        // Latenz-Ersparnis des Bypass wieder zunichtemachen. Ist raw_mouse aktiv, ist es
        // die alleinige Quelle fuer Maus-Bewegung.
        if self.raw_mouse.is_some() {
            return;
        }
        if let DeviceEvent::MouseMotion { delta } = event {
            self.handle_mouse_delta(delta);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // Tipp-Klang fuer JEDES Eingabefeld an einer Stelle: vor der Taste den Inhalt des
        // aktiven Feldes merken, danach vergleichen (s. typing_feedback).
        let vorher = match &event {
            WindowEvent::KeyboardInput { event: k, .. } if k.state == ElementState::Pressed => Some(self.typing_snapshot()),
            _ => None,
        };
        self.handle_window_event(event_loop, id, event);
        self.flush_addon_packets();
        if let Some(vorher) = vorher {
            self.typing_feedback(vorher);
        }
    }
}

// Welcher Klang zu einer Knopf-Aktion gehoert. Die Zuordnung folgt der ART der Aktion, nicht
// dem einzelnen Knopf -- achtzig Aktionen, acht Klaenge (s. ui_sound).
//
// Bei den Umschaltern entscheidet der Zustand NACH dem Klick ueber die Richtung. Deshalb
// bekommt die Funktion die Einstellungen: sie wird VOR dem Anwenden aufgerufen und dreht den
// gelesenen Wert um.
fn ui_sound_for_action(action: ButtonAction, settings: &Settings) -> ui_sound::UiSound {
    use ui_sound::UiSound;
    let umschalten = |war_an: bool| if war_an { UiSound::ToggleOff } else { UiSound::ToggleOn };
    match action {
        // Fenster auf und zu (Quick Connect, Server-Formular, Tastenfenster, Modul-Menue; dazu
        // Glocke, Acc-Manager und das Betreten der Einstellungen, s. handle_menu_click). Ein
        // Wechsel zwischen Seiten oder Einstellungs-Kategorien ist kein Fenster und klickt nur,
        // ebenso Kontenliste und Element-Fenster im HUD-Editor (Nutzerwunsch).
        ButtonAction::OpenQuickConnect | ButtonAction::NewServer | ButtonAction::OpenKeybindAdvanced(_) => UiSound::Open,
        ButtonAction::CloseModuleMenu | ButtonAction::CloseKeybindAdvanced => UiSound::Close,
        ButtonAction::OpenScreen(_) | ButtonAction::OpenLink(_) | ButtonAction::OpenShaderSettings | ButtonAction::OpenLightLevels | ButtonAction::OpenParticleSettings | ButtonAction::Back => UiSound::Click,
        // Schrittweise Werte: Stepper, Listen, Auswahl.
        ButtonAction::FrameLatencyDown
        | ButtonAction::FrameLatencyUp
        | ButtonAction::TearTimingDown
        | ButtonAction::TearTimingUp
        | ButtonAction::TearReserveDown
        | ButtonAction::TearReserveUp
        | ButtonAction::FpsLimitPrev
        | ButtonAction::FpsLimitNext
        | ButtonAction::SensitivityDown
        | ButtonAction::SensitivityUp
        | ButtonAction::SensitivityYDown
        | ButtonAction::SensitivityYUp
        | ButtonAction::CursorSensitivityDown
        | ButtonAction::CursorSensitivityUp
        | ButtonAction::CursorSizeDown
        | ButtonAction::CursorWidthDown
        | ButtonAction::CursorWidthUp
        | ButtonAction::HoldConfirmDown
        | ButtonAction::HoldConfirmUp
        | ButtonAction::ReconnectHoldDown
        | ButtonAction::ReconnectHoldUp
        | ButtonAction::CursorSizeUp
        | ButtonAction::FovDown(_)
        | ButtonAction::FovUp(_)
        | ButtonAction::CameraDistanceDown(_)
        | ButtonAction::CameraDistanceUp(_)
        | ButtonAction::CameraYOffsetDown(_)
        | ButtonAction::CameraYOffsetUp(_)
        | ButtonAction::CycleProjection
        | ButtonAction::CycleTearPosition
        | ButtonAction::CycleRenderPath
        | ButtonAction::CycleParticles
        | ButtonAction::CycleDeathAnimation
        | ButtonAction::CycleHoldUseRepeat
        | ButtonAction::CycleWallpaper
        | ButtonAction::CycleLanguage(_)
        | ButtonAction::MeshBudgetDown
        | ButtonAction::MeshBudgetUp
        | ButtonAction::MaxRenderDistanceDown
        | ButtonAction::MaxRenderDistanceUp
        | ButtonAction::ChunkUpdateThreadsDown
        | ButtonAction::ChunkUpdateThreadsUp
        | ButtonAction::CrosshairColorPrev
        | ButtonAction::CrosshairColorNext
        | ButtonAction::CrosshairBoostPrev
        | ButtonAction::CrosshairBoostNext
        | ButtonAction::CrosshairSizePrev
        | ButtonAction::CrosshairSizeNext
        | ButtonAction::InventoryScalePrev
        | ButtonAction::InventoryScaleNext
        | ButtonAction::HotbarScalePrev
        | ButtonAction::HotbarScaleNext
        | ButtonAction::ScatteringPrev
        | ButtonAction::ScatteringNext
        | ButtonAction::SunSizePrev
        | ButtonAction::SunSizeNext
        | ButtonAction::BlockSoftnessPrev
        | ButtonAction::BlockSoftnessNext
        | ButtonAction::LightmapDebugPrev
        | ButtonAction::LightmapDebugNext
        | ButtonAction::LightFactorPrev(_)
        | ButtonAction::LightFactorNext(_)
        | ButtonAction::HotbarDurabilityPrev
        | ButtonAction::HotbarDurabilityNext
        | ButtonAction::ToggleHotbarDurabilityMode
        | ButtonAction::ToggleHotbarAirBar
        | ButtonAction::EffectsDetailed(_)
        | ButtonAction::EffectsAlign(_)
        | ButtonAction::EffectsNameColor(_)
        | ButtonAction::EffectsTimerStyle(_)
        | ButtonAction::EffectsTimerBlink(_)
        | ButtonAction::EffectsTimerColor(_)
        | ButtonAction::ChatScalePrev
        | ButtonAction::ChatScaleNext
        | ButtonAction::UiPitchDown
        | ButtonAction::UiPitchUp
        | ButtonAction::InventoryTransparencyPrev
        | ButtonAction::InventoryTransparencyNext
        | ButtonAction::GlintSpeedPrev
        | ButtonAction::GlintSpeedNext
        | ButtonAction::GlintStrengthPrev
        | ButtonAction::GlintStrengthNext
        | ButtonAction::PlayerListScalePrev
        | ButtonAction::PlayerListScaleNext
        | ButtonAction::MenuBackdropPrev
        | ButtonAction::MenuBackdropNext
        | ButtonAction::ScrollInertiaPrev
        | ButtonAction::ScrollInertiaNext
        | ButtonAction::ScrollDistancePrev
        | ButtonAction::ScrollDistanceNext
        | ButtonAction::EntityDistancePrev
        | ButtonAction::EntityDistanceNext
        | ButtonAction::BlockEntityDistancePrev
        | ButtonAction::BlockEntityDistanceNext
        | ButtonAction::HitboxDensityPrev
        | ButtonAction::HitboxDensityNext
        | ButtonAction::ArrowOpacityPrev
        | ButtonAction::ArrowOpacityNext
        | ButtonAction::ColorSaturationPrev
        | ButtonAction::ColorSaturationNext
        | ButtonAction::ChunkSaverThreadsPrev
        | ButtonAction::ChunkSaverThreadsNext
        | ButtonAction::ChunkSaverThresholdPrev
        | ButtonAction::ChunkSaverThresholdNext
        | ButtonAction::ChunkSaverDistancePrev
        | ButtonAction::ChunkSaverDistanceNext
        | ButtonAction::TrailHoldPrev
        | ButtonAction::TrailHoldNext
        | ButtonAction::TrailFadePrev
        | ButtonAction::TrailFadeNext
        | ButtonAction::HitboxOpacityPrev
        | ButtonAction::HitboxOpacityNext
        | ButtonAction::DamageTintTicksPrev
        | ButtonAction::DamageTintTicksNext
        | ButtonAction::SmoothLightingDown
        | ButtonAction::SmoothLightingUp
        | ButtonAction::BlockOutlineWidthPrev
        | ButtonAction::BlockOutlineWidthNext
        | ButtonAction::ToggleKeybindMode(_) => UiSound::Step,
        // Umschalter -- Richtung aus dem Zustand VOR dem Klick.
        ButtonAction::ToggleHud => umschalten(settings.show_hud),
        ButtonAction::ToggleBlockShadows => umschalten(settings.shader_block_shadows),
        ButtonAction::ToggleCmaa2 => umschalten(settings.cmaa2_enabled),
        ButtonAction::ToggleCrosshairOutline => umschalten(settings.crosshair_outline),
        ButtonAction::ToggleCrosshairRingOutline => umschalten(settings.crosshair_ring_outline),
        ButtonAction::ToggleEffectsHud => umschalten(settings.show_effects_hud),
        ButtonAction::ToggleEffectsIcon => umschalten(settings.effects_icon),
        ButtonAction::ToggleEffectsHideAmbient => umschalten(settings.effects_hide_ambient),
        ButtonAction::ToggleEffectsName => umschalten(settings.effects_name),
        ButtonAction::ParticleToggle(i) => umschalten(settings.particle_tuning[i].enabled),
        ButtonAction::ParticleAllAtOnce(i) => umschalten(settings.particle_tuning[i].all_at_once),
        ButtonAction::ParticleStep(..) => UiSound::Step,
        ButtonAction::ToggleCaveCulling => umschalten(settings.cave_culling),
        ButtonAction::ToggleMipmapping => umschalten(settings.mipmapping),
        ButtonAction::ToggleLatinMacrons => umschalten(settings.latin_macrons),
        ButtonAction::ToggleLoanwords => umschalten(settings.english_loanwords),
        ButtonAction::ToggleEffectsInventory => umschalten(settings.show_effects_inventory),
        ButtonAction::ToggleBlockRngRotation => umschalten(settings.block_rng_rotation),
        ButtonAction::ToggleStableNumbers => umschalten(settings.stable_numbers),
        ButtonAction::ToggleCapitals => umschalten(settings.font_capitals),
        ButtonAction::ToggleShadowHalf(i) => umschalten(settings.shadow_half[i]),
        ButtonAction::ToggleUptime => umschalten(settings.topbar_uptime),
        ButtonAction::ToggleJumpCooldown => umschalten(settings.jump_cooldown_enabled),
        ButtonAction::ToggleSprintOnStart => umschalten(settings.sprint_on_start),
        ButtonAction::ToggleCapsLockBlocked => umschalten(settings.caps_lock_blocked),
        ButtonAction::ToggleInventoryCursorToSlot => umschalten(settings.inventory_cursor_to_slot),
        ButtonAction::ToggleCreativeOpTab => umschalten(settings.creative_op_tab),
        ButtonAction::ToggleNoChatClear => umschalten(settings.no_chat_clear),
        ButtonAction::ToggleAudioAutoOptimize => umschalten(settings.audio_auto_optimize),
        ButtonAction::ToggleSignEditInWorld => umschalten(settings.sign_edit_in_world),
        ButtonAction::SkipPerspective(p) => umschalten(settings.perspective_skipped(p)),
        ButtonAction::ToggleInventoryMergeHover => umschalten(settings.inventory_merge_hover),
        ButtonAction::TogglePotionMarkStrong => umschalten(settings.potion_mark_strong),
        ButtonAction::TogglePotionMarkLong => umschalten(settings.potion_mark_long),
        ButtonAction::TogglePotionMarkWeak => umschalten(settings.potion_mark_weak),
        ButtonAction::ToggleDamageTintSkinOnly => umschalten(settings.damage_tint_skin_only),
        ButtonAction::ToggleSnapTap => umschalten(settings.snap_tap_enabled),
        ButtonAction::ToggleUseWhileMining => umschalten(settings.use_while_mining),
        ButtonAction::ToggleRocketRepeat => umschalten(settings.hold_use_repeat_rockets),
        ButtonAction::ToggleExtinguishDcGhost => umschalten(settings.no_extinguish_dc_ghost),
        ButtonAction::ToggleCursorCircle => umschalten(settings.cursor_circle),
        ButtonAction::ToggleSensSplit => umschalten(settings.mouse_sens_split),
        ButtonAction::ToggleSneakEdgeProtection => umschalten(settings.sneak_edge_protection_enabled),
        ButtonAction::ToggleModuleTotem => umschalten(settings.module_totem_indicator),
        ButtonAction::ToggleModuleTotemSound => umschalten(settings.module_totem_sound),
        ButtonAction::ToggleModuleHitbox => umschalten(settings.module_hitbox_enabled),
        ButtonAction::ToggleModuleHitboxSmoothed => umschalten(settings.module_hitbox_show_smoothed),
        ButtonAction::ToggleModuleHitboxRaw => umschalten(settings.module_hitbox_show_raw),
        ButtonAction::ToggleModuleHitboxDotted => umschalten(settings.module_hitbox_dotted),
        ButtonAction::ToggleModuleHitboxItems => umschalten(settings.module_hitbox_items),
        ButtonAction::ToggleModuleHitboxSelf => umschalten(settings.module_hitbox_self),
        ButtonAction::ToggleHitboxKind(art) => umschalten(settings.module_hitbox_styles[art.index()].on),
        ButtonAction::ToggleHitboxKindOverride(art) => umschalten(settings.module_hitbox_styles[art.index()].custom),
        ButtonAction::ToggleModuleCritColor => umschalten(settings.module_crit_color_enabled),
        ButtonAction::ToggleModuleBlockOutline => umschalten(settings.module_block_outline),
        ButtonAction::ToggleBlockOutlineHideMining => umschalten(settings.block_outline_hide_mining),
        ButtonAction::SetBlockOutlineComplementary(_) => UiSound::Click,
        ButtonAction::ToggleModuleSkyColor => umschalten(settings.module_sky_color_enabled),
        ButtonAction::ToggleSkySun => umschalten(settings.sky_sun),
        ButtonAction::ToggleSkyMoon => umschalten(settings.sky_moon),
        ButtonAction::ToggleSkyStars => umschalten(settings.sky_stars),
        ButtonAction::ToggleSkyFixedTime => umschalten(settings.module_sky_fixed_time),
        ButtonAction::SkyTimePrev | ButtonAction::SkyTimeNext => UiSound::Step,
        ButtonAction::ToggleModuleWeather => umschalten(settings.module_weather),
        ButtonAction::WeatherRadiusPrev
        | ButtonAction::WeatherRadiusNext
        | ButtonAction::WeatherRadiusInnerPrev
        | ButtonAction::WeatherRadiusInnerNext => UiSound::Step,
        ButtonAction::ToggleModuleGhostTolerance => umschalten(settings.module_ghost_tolerance),
        ButtonAction::ToggleModuleCrystalOptimizer => umschalten(settings.module_crystal_optimizer),
        ButtonAction::ToggleModuleConsumableOptimizer => umschalten(settings.module_consumable_optimizer),
        ButtonAction::ToggleEatSlotLock => umschalten(settings.eat_slot_lock),
        ButtonAction::ToggleEatAggressivePrediction => umschalten(settings.eat_aggressive_prediction),
        ButtonAction::ToggleModuleUseTimer => umschalten(settings.module_use_timer),
        ButtonAction::ToggleModuleNoToolBreak => umschalten(settings.module_no_tool_break),
        ButtonAction::ToggleModuleBotcheckPass => umschalten(settings.module_botcheck_pass),
        ButtonAction::ToggleModuleDynamicFire => umschalten(settings.module_dynamic_fire),
        ButtonAction::ToggleModuleZoom => umschalten(settings.module_zoom),
        ButtonAction::ToggleModuleVoiceChat => umschalten(settings.module_voice_chat),
        ButtonAction::ToggleModuleShields => umschalten(settings.module_shields),
        ButtonAction::ToggleModuleArrows => umschalten(settings.module_arrows),
        ButtonAction::ToggleModuleArmorHud => umschalten(settings.module_armor_hud),
        ButtonAction::ToggleModuleTeams => umschalten(settings.module_teams),
        ButtonAction::TeamToggle(seite) => umschalten(settings.team_hitboxes[seite.index()].on),
        ButtonAction::TeamDotted(seite) => umschalten(settings.team_hitboxes[seite.index()].dotted),
        ButtonAction::TeamMatch(i) => umschalten(settings.team_match.get(i)),
        ButtonAction::TeamOwnColor(_) => UiSound::Click,
        ButtonAction::TeamNametag(..) => UiSound::Click,
        ButtonAction::TeamOpacity(..) | ButtonAction::TeamDensity(..) => UiSound::Step,
        ButtonAction::ToggleModuleKeystrokes => umschalten(settings.module_keystrokes),
        ButtonAction::KeysOpacityPrev | ButtonAction::KeysOpacityNext | ButtonAction::SessionOpacity(_) => UiSound::Step,
        ButtonAction::ToggleModuleSessionInfo => umschalten(settings.module_session_info),
        ButtonAction::ToggleSessionLine(bit) => umschalten(settings.session_lines & bit != 0),
        ButtonAction::ResetSession => UiSound::Click,
        ButtonAction::ArmorHudGapPrev
        | ButtonAction::ArmorHudGapNext
        | ButtonAction::ArmorHudNumberScalePrev
        | ButtonAction::ArmorHudNumberScaleNext
        | ButtonAction::ArmorHudNumberXPrev
        | ButtonAction::ArmorHudNumberXNext
        | ButtonAction::ArmorHudNumberYPrev
        | ButtonAction::ArmorHudNumberYNext => UiSound::Step,
        ButtonAction::ToggleHurtCam => umschalten(settings.hurt_cam),
        ButtonAction::ToggleShoulderCam => umschalten(settings.shoulder_cam),
        ButtonAction::ToggleShoulderCrosshair => umschalten(settings.shoulder_crosshair),
        ButtonAction::ToggleShoulderFade => umschalten(settings.shoulder_fade),
        ButtonAction::ToggleShoulderDecoupled => umschalten(settings.shoulder_decoupled),
        ButtonAction::ToggleModuleFreelook => umschalten(settings.module_freelook),
        ButtonAction::ShoulderOffsetPrev | ButtonAction::ShoulderOffsetNext => UiSound::Step,
        ButtonAction::ToggleArmorTrims => umschalten(settings.armor_trims),
        ButtonAction::ToggleViewModel => umschalten(settings.modified_view_model),
        ButtonAction::HurtCamStrengthPrev | ButtonAction::HurtCamStrengthNext | ButtonAction::FireHeightPrev | ButtonAction::FireHeightNext => UiSound::Step,
        ButtonAction::ToggleModuleShowMarkers => umschalten(settings.module_show_markers),
        ButtonAction::ToggleModuleSoundDebug => umschalten(settings.module_sound_debug),
        ButtonAction::ToggleModuleHandDebug => umschalten(settings.module_hand_debug),
        ButtonAction::ToggleModuleConnectionDebug => umschalten(settings.module_connection_debug),
        ButtonAction::ToggleModuleDelay => umschalten(settings.module_delay),
        ButtonAction::ToggleModuleDiscordStatus => umschalten(settings.module_discord_status),
        ButtonAction::ToggleModuleInputLog => umschalten(settings.module_input_log_enabled),
        ButtonAction::ToggleModuleCoralRecord => umschalten(settings.module_coral_record),
        ButtonAction::ToggleModuleAutoclicker => umschalten(settings.module_autoclicker),
        ButtonAction::ToggleModuleSaturationHud => umschalten(settings.module_saturation_hud),
        ButtonAction::SaturationHeightPrev | ButtonAction::SaturationHeightNext | ButtonAction::SaturationAlpha(..) => UiSound::Step,
        ButtonAction::ToggleModuleEgoName => umschalten(settings.module_ego_name),
        ButtonAction::EditEgoAlias | ButtonAction::CycleEgoDeco | ButtonAction::CycleEgoShadow | ButtonAction::CycleEgoSound => UiSound::Click,
        ButtonAction::EgoSpeed(_) | ButtonAction::EgoLight(..) | ButtonAction::EgoVolume(_) => UiSound::Step,
        ButtonAction::AutoclickerCpsPrev | ButtonAction::AutoclickerCpsNext => UiSound::Step,
        ButtonAction::ToggleModuleColor => umschalten(settings.module_color),
        ButtonAction::ToggleModuleSustainability => umschalten(settings.module_sustainability),
        ButtonAction::ToggleSustainBackground => umschalten(settings.sustain_background),
        ButtonAction::ToggleSustainPause => umschalten(settings.sustain_pause),
        ButtonAction::ToggleAnimFlag(bit) => umschalten(settings.anim_flags & bit != 0),
        ButtonAction::ToggleModuleChunkSaver => umschalten(settings.module_chunk_saver),
        ButtonAction::ToggleChunkSaverRenderSaved => umschalten(settings.chunk_saver_render_saved),
                ButtonAction::ToggleArrowIndicator => umschalten(settings.arrow_indicator),
        ButtonAction::ToggleArrowNumberOnly => umschalten(settings.arrow_number_only),
        ButtonAction::ToggleArmorHudGreen => umschalten(settings.armor_hud_green),
        ButtonAction::ToggleArmorHudRing => umschalten(settings.armor_hud_ring),
        ButtonAction::ToggleArrowWhite => umschalten(settings.arrow_white),
        ButtonAction::ToggleArrowTrail => umschalten(settings.arrow_trail),
        ButtonAction::ToggleShieldCdSelf => umschalten(settings.shield_cd_self),
        ButtonAction::ToggleShieldCdPlayers => umschalten(settings.shield_cd_players),
        ButtonAction::ToggleShieldHoldPlayers => umschalten(settings.shield_hold_players),
        ButtonAction::Toteming(action) => {
            use toteming::Action;
            let t = &settings.toteming;
            match action {
                Action::ToggleWarning => umschalten(t.warning),
                Action::ToggleShadow => umschalten(t.shadow),
                Action::ToggleSlots => umschalten(t.slots),
                Action::ToggleExtra => umschalten(t.extra),
                Action::ToggleSounds => umschalten(t.sounds),
                Action::ToggleVisuals => umschalten(t.visuals),
                Action::ToggleHideOwnFloating => umschalten(t.hide_own_floating),
                Action::ToggleHideOwnParticles => umschalten(t.hide_own_particles),
                Action::ToggleHideOtherParticles => umschalten(t.hide_other_particles),
                _ => UiSound::Click,
            }
        }
        // Alles Uebrige ist eine ausgefuehrte Aktion.
        _ => UiSound::Click,
    }
}

// Ein Modell-Viereck (assets) in die Mesher-Form (world): Atlas-UV je Ecke, samt der
// Flaechen-UV-Drehung, und die feste Richtungshelligkeit wie bei Vanilla-Bloecken (oben 1,0,
// unten 0,5, Nord/Sued 0,8, West/Ost 0,6 -- nach der staerksten Achse der Normalen).
fn model_face(q: &assets::ModelQuad, rect: assets::UvRect) -> world::ModelFace {
    let [u0, v0, u1, v1] = q.uv;
    // Die Ecken laufen hier im Uhrzeigersinn, bei Vanilla (FaceInfo) gegen ihn -- dessen
    // Verschiebung um +Drehung ist hier also eine um -Drehung. Mit +Drehung lief jeder
    // 90-Grad-Ausschnitt falsch herum (Kolbenseiten, Falltueren aus Packs).
    let basis = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    let uvs = std::array::from_fn(|i| {
        let [u, v] = basis[(i + 4 - q.uv_rotation as usize % 4) % 4];
        [rect[0] + (rect[2] - rect[0]) * u / 16.0, rect[1] + (rect[3] - rect[1]) * v / 16.0]
    });
    let [nx, ny, nz] = q.normal;
    let shade = if !q.shade {
        1.0
    } else if ny.abs() >= nx.abs() && ny.abs() >= nz.abs() {
        if ny >= 0.0 { 1.0 } else { 0.5 }
    } else if nz.abs() >= nx.abs() {
        0.8
    } else {
        0.6
    };
    world::ModelFace { corners: q.corners, uvs, shade }
}

// wgpu nennt den eigentlichen Grund eines Fehlers (etwa das DXGI-HRESULT, wenn die Swapchain
// nicht angelegt werden kann) nur ueber `log`; ohne Logger fehlte er im client.log.
struct WgpuLog;

impl log::Log for WgpuLog {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::Level::Error && meta.target().starts_with("wgpu")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            println!("[{}] {}", record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

fn main() {
    // Vor allem anderen: alte mc-client-Ordner auf den neuen Namen umziehen (s. assets::source).
    assets::migrate_legacy_dirs();
    if log::set_logger(&WgpuLog).is_ok() {
        log::set_max_level(log::LevelFilter::Error);
    }

    // nice -10 nur, wenn ueber scripts/run-with-performance.sh gestartet (das setzt
    // CORAL_PERFORMANCE=1). Im normalen Entwicklungs-Alltag laeuft der Client damit
    // auf Standard-Prioritaet und kann den Desktop nicht ausbremsen; die Erhoehung
    // gehoert zu den Latenz-Messungen, nicht zu jedem cargo run.
    if std::env::var_os("CORAL_PERFORMANCE").is_some() {
        latency_tuning::raise_process_priority();
    }

    // Config festlegen, bevor irgendetwas Einstellungen liest: `--config <Name>` (so startet der
    // Launcher) bzw. `--config=<Name>`, sonst die zuletzt gewaehlte.
    let mut args = std::env::args().skip(1);
    let mut config_arg = None;
    while let Some(arg) = args.next() {
        if arg == "--config" {
            config_arg = args.next();
        } else if let Some(name) = arg.strip_prefix("--config=") {
            config_arg = Some(name.to_string());
        }
    }
    let config = assets::startup_config(config_arg.as_deref());
    eprintln!("Config: {config}");
    set_active_config(&config);

    // Spieldaten aus dem Client-Jar (Entity-Modelle, Kreativ-Tabs, Standard-Components der
    // Items): extrahiert der Launcher
    // einmal in seinen Speicher (s. assets::extract_game_data), der Client liest sie nur. Ohne
    // sie stehen Dorfbewohner, Tiere, Boote ... als Kaesten da, und im Kreativmodus oeffnet
    // sich das normale Inventar.
    match assets::game_data_status(MC_VERSION) {
        assets::GameDataStatus::Missing | assets::GameDataStatus::DumperChanged => {
            eprintln!("Spieldaten fehlen oder passen nicht zu diesem Client -- bitte im Launcher (neu) extrahieren.");
        }
        status => {
            if status == assets::GameDataStatus::JarChanged {
                eprintln!("Spieldaten stammen aus einem anderen Jar -- im Launcher neu extrahieren.");
            }
            if let Some(text) = assets::stored_entity_models(MC_VERSION) {
                world::model_data::install(&text);
            }
            if let Some(text) = assets::stored_creative_tabs(MC_VERSION) {
                creative_screen::set_creative_dump(text);
            }
            // Standard-Components der Items (s. world::item_components).
            if let Some(text) = assets::stored_item_components(MC_VERSION) {
                let (items, components, fehler) = world::item_components::load(&text);
                eprintln!("[items] Standard-Components: {items} Items, {components} Components, {} nicht lesbar {fehler:?}", fehler.len());
            }
        }
    }

    // Settings schon hier laden (nicht erst in App::new): der aktive Skin muss bereits beim
    // Konstruieren von MinecraftInstallation feststehen (s. dortigen resource_pack_dir-
    // Feldkommentar) -- App::new bekommt die schon geladenen Settings durchgereicht, statt
    // sie ein zweites Mal von der Platte zu lesen.
    let settings = Settings::load();
    let (resource_pack_dirs, packs_below_vanilla) = skin_stack_dirs(&settings.active_skins, &settings.skin_exclusions, &[]);

    // Die lokale Minecraft-Installation ist Voraussetzung, nicht Kuer: Font und Texturen
    // kommen zur Laufzeit aus ihr (mitgeliefert wird nichts davon). Deshalb hier zuerst
    // pruefen -- ein Abbruch mit klarer Meldung im Terminal ist deutlich freundlicher als
    // ein Absturz, nachdem das Fenster schon den ganzen Bildschirm belegt.
    let installation = match assets::MinecraftInstallation::locate_stack(MC_VERSION, resource_pack_dirs, packs_below_vanilla) {
        Ok(installation) => installation,
        Err(e) => {
            eprintln!("Coral Client braucht eine lokale Minecraft-Installation von {MC_VERSION}.");
            eprintln!("  Problem: {e}");
            eprintln!("  Die Version muss einmal über den offiziellen oder den Prism-Launcher gestartet worden");
            eprintln!("  sein, damit Jar und Assets lokal vorliegen. Welche Installation benutzt wird, stellt");
            eprintln!("  man im coral-client-launcher ein (oder per CORAL_MINECRAFT_DIR).");
            std::process::exit(1);
        }
    };

    let event_loop = EventLoop::new().expect("Event-Loop konnte nicht erstellt werden");
    event_loop.set_control_flow(ControlFlow::Wait);

    // Das Fenster kommt sofort, die Assets baut ein Thread (s. loading_screen).
    let mut start = loading_screen::Startup::new(installation, settings);
    event_loop.run_app(&mut start).expect("Event-Loop-Fehler");
}

#[cfg(test)]
mod main_tests;
