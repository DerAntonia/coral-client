// Module: Kennungen, Kategorien, Addon-Module, Detail-Layout.

use crate::*;

// Welches Modul im Modul-Menue gerade in der rechten Spalte konfiguriert wird (s.
// App::build_module_menu). Nur die linke Liste braucht diese Unterscheidung -- neue Module
// kommen hier als weitere Variante dazu, nicht als neues Layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModuleId {
    Totem,
    TotemWarning,
    TotemSlots,
    PopSounds,
    PopVisuals,
    Hitbox,
    Teams,
    SkyColor,
    Weather,
    GhostTolerance,
    CrystalOptimizer,
    ConsumableOptimizer,
    UseTimer,
    NoToolBreak,
    BotcheckPass,
    Shields,
    Arrows,
    ArmorHud,
    Keystrokes,
    SessionInfo,
    DynamicFire,
    Zoom,
    Freelook,
    VoiceChat,
    BlockOutline,
    ShowMarkers,
    SoundDebug,
    HandDebug,
    ConnectionDebug,
    Delay,
    FpsDisplay,
    DiscordStatus,
    InputLog,
    CoralRecord,
    Autoclicker,
    SaturationHud,
    EgoName,
    Color,
    Sustainability,
    ChunkSaver,
    // Modul eines Addons: Index in ADDON_MODULES.
    Addon(u16),
}

// Was ModuleId und ModuleCategory ueber die Module der Addons wissen muessen, ohne an die App
// zu kommen: Name und Beschreibung (einmal festgehalten, s. intern), Kategorie (Index in
// ADDON_CATEGORIES), Schluessel in der Config und wo das Modul in App::addons steht.
pub(crate) struct AddonModuleInfo {
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) category: u16,
    pub(crate) key: String,
    pub(crate) addon: usize,
    pub(crate) module: usize,
}

pub(crate) static ADDON_MODULES: std::sync::RwLock<Vec<AddonModuleInfo>> = std::sync::RwLock::new(Vec::new());
pub(crate) static ADDON_CATEGORIES: std::sync::RwLock<Vec<&'static str>> = std::sync::RwLock::new(Vec::new());

// Text, der so lange lebt wie der Prozess. Jeder Text nur einmal -- meldet sich ein Addon neu
// an, waechst nichts nach.
pub(crate) fn intern(s: &str) -> &'static str {
    static TEXTE: std::sync::Mutex<Option<std::collections::HashSet<&'static str>>> = std::sync::Mutex::new(None);
    let mut texte = TEXTE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let texte = texte.get_or_insert_with(Default::default);
    if let Some(t) = texte.get(s) {
        return t;
    }
    let t: &'static str = Box::leak(s.to_string().into_boxed_str());
    texte.insert(t);
    t
}

pub(crate) fn addon_module_info<T>(i: u16, f: impl FnOnce(&AddonModuleInfo) -> T) -> Option<T> {
    ADDON_MODULES.read().ok()?.get(i as usize).map(f)
}

pub(crate) const MODULE_IDS: [ModuleId; 40] = [
    ModuleId::Totem,
    ModuleId::TotemWarning,
    ModuleId::TotemSlots,
    ModuleId::PopSounds,
    ModuleId::PopVisuals,
    ModuleId::Hitbox,
    ModuleId::Teams,
    ModuleId::SkyColor,
    ModuleId::Weather,
    ModuleId::GhostTolerance,
    ModuleId::CrystalOptimizer,
    ModuleId::ConsumableOptimizer,
    ModuleId::NoToolBreak,
    ModuleId::BotcheckPass,
    ModuleId::Shields,
    ModuleId::UseTimer,
    ModuleId::Arrows,
    ModuleId::ArmorHud,
    ModuleId::Keystrokes,
    ModuleId::SessionInfo,
    ModuleId::DynamicFire,
    ModuleId::Zoom,
    ModuleId::Freelook,
    ModuleId::VoiceChat,
    ModuleId::BlockOutline,
    ModuleId::SaturationHud,
    ModuleId::EgoName,
    ModuleId::ShowMarkers,
    ModuleId::SoundDebug,
    ModuleId::HandDebug,
    ModuleId::ConnectionDebug,
    ModuleId::Delay,
    ModuleId::FpsDisplay,
    ModuleId::Autoclicker,
    ModuleId::DiscordStatus,
    ModuleId::InputLog,
    ModuleId::CoralRecord,
    ModuleId::Color,
    ModuleId::Sustainability,
    ModuleId::ChunkSaver,
];

impl ModuleId {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Addon(i) => addon_module_info(i, |m| m.label).unwrap_or("Addon"),
            Self::Totem => "Totem-Anzeige",
            Self::TotemWarning => "Totem-Warnung",
            Self::TotemSlots => "Totem-Vorschau",
            Self::PopSounds => "Pop-Klänge",
            Self::PopVisuals => "Pop-Optik",
            Self::Hitbox => "Hitbox-Anzeige",
            Self::Teams => "Teams",
            Self::SkyColor => "Himmel",
            Self::Weather => "Wetter",
            Self::GhostTolerance => "Ghostblock-Indikator",
            Self::CrystalOptimizer => "Crystal-Optimizer",
            Self::ConsumableOptimizer => "Usage-Optimizer",
            Self::UseTimer => "Timer",
            Self::NoToolBreak => "No Tool Break",
            Self::BotcheckPass => "Botcheck-Pass",
            Self::Shields => "Schilde",
            Self::Arrows => "Pfeile",
            Self::ArmorHud => "Rüstung",
            Self::Keystrokes => "Keystrokes",
            Self::SessionInfo => "Session-Info",
            Self::DynamicFire => "Dynamisches Feuer",
            Self::Zoom => "Zoom",
            Self::Freelook => "Freelook",
            Self::VoiceChat => "Sprachchat",
            Self::BlockOutline => "Blockumriss",
            Self::ShowMarkers => "Barrieren anzeigen",
            Self::SoundDebug => "Sound-Debug",
            Self::HandDebug => "Hand-Debug",
            Self::ConnectionDebug => "Connection Debug",
            Self::Delay => "Delay",
            Self::FpsDisplay => "FPS-Anzeige",
            Self::DiscordStatus => "Discord-Status",
            Self::InputLog => "Input-Log",
            Self::CoralRecord => "Coral-Record",
            Self::Autoclicker => "Autoklicker",
            Self::SaturationHud => "Sättigung",
            Self::EgoName => "Eigenname",
            Self::Color => "Farbe",
            Self::Sustainability => "Nachhaltigkeit",
            Self::ChunkSaver => "Chunk-Saver",
        }
    }

    // Der Schalter der linken Liste -- das MASTER-Ein/Aus des Moduls, nicht seine
    // Unter-Einstellungen (die stehen nur rechts, im Detail des ausgewaehlten Moduls).
    pub(crate) fn enabled(self, settings: &Settings) -> bool {
        match self {
            Self::Addon(i) => addon_module_info(i, |m| settings.addon_values.get(&format!("{}/enabled", m.key)).is_some_and(|v| v == "true")).unwrap_or(false),
            Self::Totem => settings.module_totem_indicator,
            Self::TotemWarning => settings.toteming.warning,
            Self::TotemSlots => settings.toteming.slots,
            Self::PopSounds => settings.toteming.sounds,
            Self::PopVisuals => settings.toteming.visuals,
            Self::Hitbox => settings.module_hitbox_enabled,
            Self::Teams => settings.module_teams,
            Self::SkyColor => settings.module_sky_color_enabled,
            Self::Weather => settings.module_weather,
            Self::GhostTolerance => settings.module_ghost_tolerance,
            Self::CrystalOptimizer => settings.module_crystal_optimizer,
            Self::ConsumableOptimizer => settings.module_consumable_optimizer,
            Self::UseTimer => settings.module_use_timer,
            Self::NoToolBreak => settings.module_no_tool_break,
            Self::BotcheckPass => settings.module_botcheck_pass,
            Self::Shields => settings.module_shields,
            Self::Arrows => settings.module_arrows,
            Self::ArmorHud => settings.module_armor_hud,
            Self::Keystrokes => settings.module_keystrokes,
            Self::SessionInfo => settings.module_session_info,
            Self::DynamicFire => settings.module_dynamic_fire,
            Self::Zoom => settings.module_zoom,
            Self::Freelook => settings.module_freelook,
            Self::VoiceChat => settings.module_voice_chat,
            Self::BlockOutline => settings.module_block_outline,
            Self::ShowMarkers => settings.module_show_markers,
            Self::SoundDebug => settings.module_sound_debug,
            Self::HandDebug => settings.module_hand_debug,
            Self::ConnectionDebug => settings.module_connection_debug,
            Self::Delay => settings.module_delay,
            Self::FpsDisplay => settings.show_hud,
            Self::DiscordStatus => settings.module_discord_status,
            Self::InputLog => settings.module_input_log_enabled,
            Self::CoralRecord => settings.module_coral_record,
            Self::Autoclicker => settings.module_autoclicker,
            Self::SaturationHud => settings.module_saturation_hud,
            Self::EgoName => settings.module_ego_name,
            Self::Color => settings.module_color,
            Self::Sustainability => settings.module_sustainability,
            Self::ChunkSaver => settings.module_chunk_saver,
        }
    }

    pub(crate) const fn toggle_action(self) -> ButtonAction {
        match self {
            Self::Addon(i) => ButtonAction::ToggleAddonModule(i),
            Self::Totem => ButtonAction::ToggleModuleTotem,
            Self::TotemWarning => ButtonAction::Toteming(toteming::Action::ToggleWarning),
            Self::TotemSlots => ButtonAction::Toteming(toteming::Action::ToggleSlots),
            Self::PopSounds => ButtonAction::Toteming(toteming::Action::ToggleSounds),
            Self::PopVisuals => ButtonAction::Toteming(toteming::Action::ToggleVisuals),
            Self::Hitbox => ButtonAction::ToggleModuleHitbox,
            Self::Teams => ButtonAction::ToggleModuleTeams,
            Self::SkyColor => ButtonAction::ToggleModuleSkyColor,
            Self::Weather => ButtonAction::ToggleModuleWeather,
            Self::GhostTolerance => ButtonAction::ToggleModuleGhostTolerance,
            Self::CrystalOptimizer => ButtonAction::ToggleModuleCrystalOptimizer,
            Self::ConsumableOptimizer => ButtonAction::ToggleModuleConsumableOptimizer,
            Self::UseTimer => ButtonAction::ToggleModuleUseTimer,
            Self::NoToolBreak => ButtonAction::ToggleModuleNoToolBreak,
            Self::BotcheckPass => ButtonAction::ToggleModuleBotcheckPass,
            Self::Shields => ButtonAction::ToggleModuleShields,
            Self::Arrows => ButtonAction::ToggleModuleArrows,
            Self::ArmorHud => ButtonAction::ToggleModuleArmorHud,
            Self::Keystrokes => ButtonAction::ToggleModuleKeystrokes,
            Self::SessionInfo => ButtonAction::ToggleModuleSessionInfo,
            Self::DynamicFire => ButtonAction::ToggleModuleDynamicFire,
            Self::Zoom => ButtonAction::ToggleModuleZoom,
            Self::Freelook => ButtonAction::ToggleModuleFreelook,
            Self::VoiceChat => ButtonAction::ToggleModuleVoiceChat,
            Self::BlockOutline => ButtonAction::ToggleModuleBlockOutline,
            Self::ShowMarkers => ButtonAction::ToggleModuleShowMarkers,
            Self::SoundDebug => ButtonAction::ToggleModuleSoundDebug,
            Self::HandDebug => ButtonAction::ToggleModuleHandDebug,
            Self::ConnectionDebug => ButtonAction::ToggleModuleConnectionDebug,
            Self::Delay => ButtonAction::ToggleModuleDelay,
            Self::FpsDisplay => ButtonAction::ToggleHud,
            Self::DiscordStatus => ButtonAction::ToggleModuleDiscordStatus,
            Self::InputLog => ButtonAction::ToggleModuleInputLog,
            Self::CoralRecord => ButtonAction::ToggleModuleCoralRecord,
            Self::Autoclicker => ButtonAction::ToggleModuleAutoclicker,
            Self::SaturationHud => ButtonAction::ToggleModuleSaturationHud,
            Self::EgoName => ButtonAction::ToggleModuleEgoName,
            Self::Color => ButtonAction::ToggleModuleColor,
            Self::Sustainability => ButtonAction::ToggleModuleSustainability,
            Self::ChunkSaver => ButtonAction::ToggleModuleChunkSaver,
        }
    }

    // Einzeiler unter dem Modulnamen in der Detailspalte.
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Addon(i) => addon_module_info(i, |m| m.description).unwrap_or(""),
            Self::Totem => "Hotbar = gold/rechts, Zweithand = blau/links",
            Self::TotemWarning => "X am Fadenkreuz, wenn ein Totem nicht an seinem Platz liegt",
            Self::TotemSlots => "Zeigt schon vor dem Öffnen, wo im Inventar die Totems liegen",
            Self::PopSounds => "Klänge für Pops, Explosion und Anker -- ersetzt den Signalton",
            Self::PopVisuals => "Flug-Totem und Partikelwolken beim Pop abschalten",
            Self::Hitbox => "Trefferbox anderer Entities: geglättet und/oder roh",
            Self::Teams => "Eigene Hitboxen für Verbündete und Gegner -- gleiche Namensfarbe wie deine = gleiches Team",
            Self::SkyColor => "Einfarbig, Verlauf mit Sonne und Mond oder der Himmel aus dem Skin, dazu eine feste Uhrzeit -- aus: Tag/Nacht wie gehabt",
            Self::Weather => "Regen oder Schnee fest einstellen (oder klar) und in welcher Kugel um dich er fällt (von/bis) -- nur fürs Bild, auch wenn der Pack Wetter ausblendet",
            Self::GhostTolerance => "Unbestätigte Blöcke (gesetzt, abgebaut, Anker) werden nach der Toleranz halbdurchsichtig",
            Self::CrystalOptimizer => "Getroffene Kristalle sofort weg, ohne auf den Server zu warten",
            Self::ConsumableOptimizer => "Essen und Trinken enden im richtigen Tick, ohne auf den Server zu warten -- der Bogen spannt zu kurzes Laden durch",
            Self::UseTimer => "Kreis am Fadenkreuz: füllt sich beim Essen bis fertig, leert sich beim Spannen bis zum kritischen Pfeil",
            Self::NoToolBreak => "Sperrt das Abbauen, bevor ein Diamant-/Netherit-Werkzeug mit Reparatur zerbricht",
            Self::BotcheckPass => "Beim Betreten wie Vanilla erst die Welt laden, dann loslaufen (langsamer)",
            Self::Shields => "Schild-Cooldown (eigener und fremder) und gehobene Schilde anderer Spieler",
            Self::Arrows => "Munitionsanzeige bei Bogen/Armbrust und weiße Flugbahnen hinter Pfeilen",
            Self::ArmorHud => "Getragene Rüstung mit Haltbarkeit -- verschieben im HUD-Editor",
            Self::Keystrokes => "Bewegungstasten, Springen und Maustasten -- verschieben im HUD-Editor",
            Self::SessionInfo => "Spielzeit seit dem Start, dazu Kills (aus der Todesmeldung im Chat), Tode und Siege -- verschieben im HUD-Editor",
            Self::DynamicFire => "Mit Feuerresistenz blenden Feuerblöcke in einer Sekunde aus, danach wieder ein",
            Self::Zoom => "Verengt das Sichtfeld, solange die Zoom-Taste (Steuerung) aktiv ist",
            Self::Freelook => "Solange die Freelook-Taste (Steuerung) aktiv ist, dreht die Maus nur die Kamera, der Spieler schaut weiter geradeaus",
            Self::VoiceChat => "Sprachchat wie Simple Voice Chat: Spieler in der Nähe hören, sprechen mit der Sprechtaste (Steuerung)",
            Self::BlockOutline => "Farbe, Deckkraft und Breite des Rahmens um den anvisierten Block",
            Self::ShowMarkers => "Barrieren, Lichtblöcke und Strukturleere sichtbar machen",
            Self::SoundDebug => "Zwei Balken: wie laut gerade der linke und der rechte Kanal ist",
            Self::HandDebug => "Verkleinert das Bild nach links oben, Hand und Items ragen ungeschnitten über den schwarzen Rand",
            Self::ConnectionDebug => "Benachrichtigung bei Lag-back und starkem Rubberbanding",
            Self::Delay => "Einstellbare Verzögerung je Richtung auf der Leitung -- nur auf lokalen Servern und anticheat-test.com",
            Self::FpsDisplay => "Bildrate und Frame-Zeiten -- verschieben im HUD-Editor",
            Self::DiscordStatus => "Zeigt in Discord, dass du Coral Client spielst -- mit Version, Server und Spielzeit",
            Self::SaturationHud => "Balken über der Hungerleiste: wie viel Sättigung noch übrig ist, von rechts nach links",
            Self::EgoName => "Der eigene Name überall im Text umgeschrieben und eingefärbt, auf Wunsch mit Ton, wenn jemand ihn schreibt",
            Self::Autoclicker => "Klickt beim Halten von Angreifen oder Benutzen von selbst -- nur auf anticheat-test.com und lokalen Servern",
            Self::InputLog => "Schreibt Tasten, Maustasten und Rad (nur im Spiel, nicht Chat) mit Zeitstempel in input_log.txt",
            Self::CoralRecord => "Zeichnet Pakete, Kamera und Maus auf, um später ein Video daraus zu rendern (recordings/*.crec). Auch der Chat landet in der Aufnahme. Mitten im Spiel eingeschaltet, speichert es zuerst den aktuellen Zustand",
            Self::Color => "Sättigung des ganzen Bildes -- 0 % ist schwarzweiß",
            Self::Sustainability => "Spart Strom: keine Bilder, solange das Fenster im Hintergrund ist, und eine stehende Welt hinter dem Pausemenü",
            Self::ChunkSaver => "Speichert jeden geladenen Chunk -- daraus zeichnet die LOD-Render-Distance (Einstellungen > Video) Fernsicht weit über die Render-Distance hinaus (wie Voxy)",
        }
    }
}

// Zustand des Autoklickers (s. App::tick_autoclicker): je ein Kanal fuer Angreifen und
// Benutzen (s. AUTOCLICK_ACTIONS).
#[derive(Default)]
pub(crate) struct Autoclick {
    pub(crate) channels: [AutoclickChannel; 2],
    // Waehrend ein erzeugter Klick durch apply_key_action laeuft (nicht abfangen).
    pub(crate) synthetic: bool,
}

pub(crate) const AUTOCLICK_ACTIONS: [KeyBindAction; 2] = [KeyBindAction::Attack, KeyBindAction::Use];

#[derive(Default)]
pub(crate) struct AutoclickChannel {
    // Die Taste ist gerade gehalten (die echte, nicht die erzeugten Klicks).
    pub(crate) held: bool,
    // Ein erzeugter Klick ist unten; losgelassen wird zu release_at.
    pub(crate) down: bool,
    // Faellig ab: der naechste Druck bzw. das Loslassen des laufenden.
    pub(crate) next: Option<Instant>,
    pub(crate) release_at: Option<Instant>,
}

// Die Kategorien links im Modul-Menue. Jedes Modul gehoert zu genau einer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModuleCategory {
    CrystalPvp,
    Render,
    // Was etwas anzeigt: HUD-Elemente, Hinweise am Fadenkreuz, Hitboxen.
    Display,
    // Was etwas hoerbar macht.
    Akustik,
    Utility,
    Debug,
    // Die Kategorie eines Addons: Index in ADDON_CATEGORIES.
    Addon(u16),
}

impl ModuleCategory {
    pub(crate) const ALL: [Self; 6] =
        [Self::CrystalPvp, Self::Render, Self::Display, Self::Akustik, Self::Utility, Self::Debug];

    // Die eingebauten Kategorien, danach je angemeldetem Addon eine.
    pub(crate) fn all() -> Vec<Self> {
        let addons = ADDON_CATEGORIES.read().map_or(0, |c| c.len());
        Self::ALL.into_iter().chain((0..addons as u16).map(ModuleCategory::Addon)).collect()
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CrystalPvp => "Crystal PvP",
            Self::Render => "Render",
            Self::Display => "Display",
            Self::Akustik => "Akustik",
            Self::Utility => "Utility",
            Self::Debug => "Debug",
            Self::Addon(i) => ADDON_CATEGORIES.read().ok().and_then(|c| c.get(i as usize).copied()).unwrap_or("Addon"),
        }
    }

    // Die Module dieser Kategorie, in der Reihenfolge von MODULE_IDS bzw. der Anmeldung.
    pub(crate) fn modules(self) -> Vec<ModuleId> {
        if let Self::Addon(k) = self {
            let module = ADDON_MODULES.read().map(|m| m.iter().enumerate().filter(|(_, m)| m.category == k).map(|(i, _)| ModuleId::Addon(i as u16)).collect());
            return module.unwrap_or_default();
        }
        MODULE_IDS.iter().copied().filter(|id| id.category() == self).collect()
    }
}

impl ModuleId {
    pub(crate) fn category(self) -> ModuleCategory {
        match self {
            Self::Addon(i) => ModuleCategory::Addon(addon_module_info(i, |m| m.category).unwrap_or(0)),
            Self::PopVisuals | Self::CrystalOptimizer => ModuleCategory::CrystalPvp,
            Self::SkyColor
            | Self::Weather
            | Self::DynamicFire
            | Self::BlockOutline
            | Self::Color
            | Self::ChunkSaver => ModuleCategory::Render,
            Self::Totem
            | Self::TotemWarning
            | Self::TotemSlots
            | Self::Hitbox
            | Self::Teams
            | Self::Shields
            | Self::UseTimer
            | Self::Arrows
            | Self::ArmorHud
            | Self::Keystrokes
            | Self::SessionInfo
            | Self::SaturationHud => ModuleCategory::Display,
            Self::PopSounds | Self::VoiceChat => ModuleCategory::Akustik,
            Self::EgoName
            | Self::NoToolBreak
            | Self::ConsumableOptimizer
            | Self::Zoom
            | Self::Freelook
            | Self::DiscordStatus
            | Self::InputLog
            | Self::CoralRecord
            | Self::Sustainability => ModuleCategory::Utility,
            Self::GhostTolerance
            | Self::BotcheckPass
            | Self::ShowMarkers
            | Self::SoundDebug
            | Self::HandDebug
            | Self::ConnectionDebug
            | Self::Delay
            | Self::FpsDisplay
            | Self::Autoclicker => ModuleCategory::Debug,
        }
    }
}

// Eine Zeile in der Detailspalte des Modul-Menues, als DATEN -- Trefferpruefung
// (module_menu_buttons) und Zeichnen (build_module_menu) lesen dieselbe Liste, statt zwei
// spiegelbildliche Layouts von Hand synchron zu halten (s. Modul-Menue-Kommentar).
pub(crate) enum ModuleRow {
    Toggle { label: String, action: ButtonAction, on: bool },
    // Mit `slider` (Wert, Minimum, Maximum) ein Schieberegler wie in den Einstellungen, ohne
    // eine Auswahl mit Pfeilen.
    Stepper { label: String, prev: ButtonAction, next: ButtonAction, text: String, can_prev: bool, can_next: bool, slider: Option<(f64, f64, f64)> },
    // Farbfeld, das zugleich der Knopf zum Auf-/Zuklappen des Farbeditors ist.
    Swatch { label: String, action: ButtonAction, rgb: [f32; 3] },
    // Auswahl zwischen wenigen benannten Werten: ein breiter Knopf mit dem aktuellen Wert,
    // jeder Klick schaltet weiter.
    Choice { label: String, action: ButtonAction, text: String },
    // Genau einer von mehreren: je Wert ein Knopf nebeneinander, der gewaehlte in der
    // Akzentflaeche. Je Wert (Name, Aktion, die ihn waehlt, gewaehlt?).
    // Optional rechts daneben ein Farbfeld (Aktion, Farbe), das nur mit dem letzten Wert wirkt
    // (Einfarbig).
    Pick { label: String, options: Vec<(&'static str, ButtonAction, bool)>, swatch: Option<(ButtonAction, [f32; 3])> },
    // Schalter mit seinem Farbfeld rechts daneben in derselben Zeile.
    ToggleSwatch { label: String, action: ButtonAction, on: bool, swatch: ButtonAction, rgb: [f32; 3] },
    // Zwei Schalter nebeneinander, dahinter ein Farbfeld (s. Heading::columns fuer die Koepfe).
    TwoTogglesSwatch { label: String, first: (ButtonAction, bool), second: (ButtonAction, bool), swatch: ButtonAction, rgb: [f32; 3] },
    // Ein Knopf (z.B. "Automatisch") und dahinter Farbfelder zur Wahl -- je Farbe (Aktion,
    // Farbe, gewaehlt?). Der gewaehlte Eintrag ist hervorgehoben.
    ColorPicks { label: String, first: (&'static str, ButtonAction, bool), colors: Vec<(ButtonAction, [f32; 3], bool)> },
    // Unterkategorie: Trennlinie mit Namen wie zwischen den Abschnitten der Einstellungen,
    // darunter optional Spaltenkoepfe ueber den Schaltern der folgenden Zeilen.
    Heading { label: String, columns: &'static [&'static str] },
}

impl ModuleRow {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Toggle { label, .. }
            | Self::Stepper { label, .. }
            | Self::Swatch { label, .. }
            | Self::Choice { label, .. }
            | Self::Pick { label, .. }
            | Self::ToggleSwatch { label, .. }
            | Self::TwoTogglesSwatch { label, .. }
            | Self::ColorPicks { label, .. }
            | Self::Heading { label, .. } => label,
        }
    }
}

// Geometrie der Detailspalte: dieselben Masse wie auf den Einstellungsseiten (Zeilenhoehe,
// Abstand Beschriftung -> Bedienelement, Schalter-, Regler- und Farbfeldgroesse), nur an die
// Tafel verschoben. Die Zeilennummer `n` ist SICHTBAR-relativ (0 = oberste gerade angezeigte
// Zeile) -- das Scrollen steckt schon in der Auswahl der Zeilen.
pub(crate) struct ModuleDetailLayout {
    // Linke Kante der Beschriftungen.
    pub(crate) x: f32,
    // Oberkante der ersten sichtbaren Zeile.
    pub(crate) top: f32,
}

impl ModuleDetailLayout {
    pub(crate) fn row_y(&self, n: f32) -> f32 {
        self.top + n * SETTINGS_ROW_H
    }

    pub(crate) fn ctrl_x(&self) -> f32 {
        self.x + SETTINGS_CTRL_X - SETTINGS_LABEL_X
    }

    // Wertfeld eines Reglers oder einer Auswahl mit Pfeilen (wie stepper_value_rect).
    pub(crate) fn value_rect(&self, n: f32) -> Rect {
        Rect { x: self.ctrl_x() + STEPPER_BTN_W + STEPPER_GAP, y: self.row_y(n), w: STEPPER_VALUE_W, h: SETTINGS_CTRL_H }
    }

    // An/Aus-Schalter (wie settings_ctrl_rect).
    pub(crate) fn control_rect(&self, n: f32) -> Rect {
        Rect { x: self.ctrl_x(), y: self.row_y(n), w: TOGGLE_W, h: SETTINGS_CTRL_H }
    }

    // Knopf einer Auswahlzeile: so breit wie ein Regler (wie settings_wide_rect).
    pub(crate) fn choice_rect(&self, n: f32) -> Rect {
        Rect { w: STEPPER_VALUE_W + 2.0 * (STEPPER_BTN_W + STEPPER_GAP), ..self.control_rect(n) }
    }

    pub(crate) fn swatch_rect(&self, n: f32) -> Rect {
        Rect { w: SWATCH_W, ..self.control_rect(n) }
    }

    // Farbfeld rechts neben einer Auswahl (ModuleRow::Pick mit Farbe).
    pub(crate) fn swatch_after_choice_rect(&self, n: f32) -> Rect {
        let wahl = self.choice_rect(n);
        Rect { x: wahl.x + wahl.w + STEPPER_GAP, ..self.swatch_rect(n) }
    }

    // ModuleRow::ColorPicks: der Knopf vorne, dann die Farbfelder (i ab 0) dicht dahinter.
    pub(crate) fn color_picks_first_rect(&self, n: f32) -> Rect {
        Rect { w: COLOR_PICK_FIRST_W, ..self.control_rect(n) }
    }

    pub(crate) fn color_pick_rect(&self, n: f32, i: usize) -> Rect {
        let x = self.ctrl_x() + COLOR_PICK_FIRST_W + STEPPER_GAP + i as f32 * (COLOR_PICK_W + COLOR_PICK_GAP);
        Rect { x, w: COLOR_PICK_W, ..self.control_rect(n) }
    }

    // Farbfeld rechts neben einem Schalter (ModuleRow::ToggleSwatch).
    pub(crate) fn swatch_after_toggle_rect(&self, n: f32) -> Rect {
        Rect { x: self.ctrl_x() + TOGGLE_W + STEPPER_GAP, ..self.swatch_rect(n) }
    }

    // Der zweite Schalter einer Zeile und das Farbfeld dahinter (ModuleRow::TwoTogglesSwatch).
    pub(crate) fn second_toggle_rect(&self, n: f32) -> Rect {
        Rect { x: self.ctrl_x() + TOGGLE_W + STEPPER_GAP, ..self.control_rect(n) }
    }

    pub(crate) fn swatch_after_two_toggles_rect(&self, n: f32) -> Rect {
        Rect { x: self.ctrl_x() + 2.0 * (TOGGLE_W + STEPPER_GAP), ..self.swatch_rect(n) }
    }

    // Senkrechte Lage einer Beschriftung in Zeile `n`, mittig zum Bedienelement.
    pub(crate) fn label_y(&self, gpu: &GpuState, n: f32) -> f32 {
        (self.row_y(n) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round()
    }
}

// Auswahl zwischen zwei Werten, die bisher ein Knopf zum Durchschalten war: beide Knoepfe
// tragen dieselbe Umschalt-Aktion, der gewaehlte wird zu ButtonAction::Nothing.
pub(crate) fn pick_two(label: &str, action: ButtonAction, names: [&'static str; 2], gewaehlt: usize) -> ModuleRow {
    ModuleRow::Pick { label: label.to_string(), options: vec![(names[0], action, gewaehlt == 0), (names[1], action, gewaehlt == 1)], swatch: None }
}

// Knopf `i` von `k` nebeneinander in `r`, mit STEPPER_GAP dazwischen.
pub(crate) fn pick_split(r: Rect, i: usize, k: usize) -> Rect {
    let w = ((r.w - (k - 1) as f32 * STEPPER_GAP) / k as f32).floor();
    Rect { x: r.x + i as f32 * (w + STEPPER_GAP), w, ..r }
}

// Die Knoepfe einer Auswahl "genau einer von mehreren" in `r`: der gewaehlte in der
// Akzentflaeche und ohne Wirkung, die anderen waehlen sich selbst.
pub(crate) fn pick_buttons(r: Rect, options: &[(&'static str, ButtonAction, bool)], enabled: bool) -> Vec<Button> {
    options
        .iter()
        .enumerate()
        .map(|(i, &(name, action, gewaehlt))| Button {
            action: if gewaehlt { ButtonAction::Nothing } else { action },
            rect: pick_split(r, i, options.len()),
            label: name.to_string(),
            enabled,
            color: if gewaehlt { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
            scale: UI_SCALE_SMALL,
            slider: None,
        })
        .collect()
}

// ModuleRow::ColorPicks: Breite des vorderen Knopfs, der Farbfelder und ihr Abstand. Sechs
// Felder passen neben den Knopf in die Detailspalte.
pub(crate) const COLOR_PICK_FIRST_W: f32 = 140.0;
pub(crate) const COLOR_PICK_W: f32 = 44.0;
pub(crate) const COLOR_PICK_GAP: f32 = 8.0;
pub(crate) const COLOR_PICK_MAX: usize = 6;

// Breite der Detailspalte: Beschriftung, Regler und Platz fuer die Einheit dahinter.
pub(crate) const MODULE_DETAIL_PX_W: f32 = SETTINGS_CTRL_X - SETTINGS_LABEL_X + SLIDER_SPAN_W + 72.0;
// Rand der Detailansicht links/rechts, Kopf (Zurueck, Name, Beschreibung) und Rand unten.
pub(crate) const MODULE_DETAIL_PX_PAD: f32 = 28.0;
pub(crate) const MODULE_DETAIL_PX_HEAD: f32 = 112.0;
pub(crate) const MODULE_DETAIL_PX_FOOT: f32 = 16.0;
