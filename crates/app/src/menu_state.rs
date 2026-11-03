// Menue-Zustand: Bildschirme, Suche, HUD-Elemente, editierbare Werte, Farbwahl, Themes,
// Regler.

use crate::*;

// Ergebnis des letzten automatischen Speicherns. Wird dauerhaft angezeigt: ein
// fehlgeschlagenes Speichern darf nicht unbemerkt bleiben, sonst sind die Einstellungen
// beim naechsten Start still wieder weg.
pub(crate) enum SaveStatus {
    Saved,
    Failed(String),
}

// Welcher Menue-Screen gerade sichtbar ist. Main ist zweigeteilt (s. build_menu_ui):
// getrennt -> Startbildschirm (Verbinden/Einstellungen), verbunden -> Esc-Pausemenue
// (HUD-Editor/Disconnect/Einstellungen). Settings/HudEditor sind Unterscreens mit
// Zurueck-Navigation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuScreen {
    Main,
    // Verbindungs-Screen: erst hier erscheint die Adress-Eingabe. Vom Startbildschirm
    // ueber "Verbinden" erreichbar; "Los" startet die Verbindung und springt sofort in
    // die 3D-Ansicht (das Overlay dort zeigt die Connect-Phase, s. build_connecting_overlay).
    Connect,
    // Konto-Verwaltung (eigener Screen statt eingequetscht auf dem Connect-Screen): Microsoft-
    // Login, Refresh-Token, manuelles Token, Abmelden.
    Account,
    // Settings ist die Kategorie-Auswahl (nur Kacheln, s. build_settings_hub) -- die
    // eigentlichen Zeilen stecken in den Unterscreens darunter. SettingsGeneral traegt die
    // technischen/Performance-Einstellungen
    // (bisher direkt in Settings), die uebrigen sind Kategorie-Platzhalter fuer alles, was
    // ein vollstaendiger Client noch braucht, aber hier noch nicht (oder nur teilweise)
    // implementiert ist.
    Settings,
    SettingsGeneral,
    // Steuerung: linke Spalte = Tastenbelegung-Tabelle (Aktion/Taste/Halten-Umschalten,
    // scrollbar, s. build_settings_controls_tab), rechte Spalte = uebrige
    // Steuerungs-Einstellungen (Maus-Empfindlichkeit, Verhaltens-Platzhalter). War vorher
    // zwei Screens (Steuerung + eigener Keybinds-Screen) -- jetzt ein gemeinsamer Screen.
    SettingsControls,
    SettingsVideo,
    SettingsAudio,
    // Resourcepack-Auswahl (s. build_settings_skin_tab) -- eigener Tab statt einer Zeile im
    // General-Tab, damit "Skin"/"Texturepack" als eigenstaendiger Bereich auffindbar ist.
    SettingsSkin,
    // Inventar: bisher nur die Abdunklung hinter dem offenen Inventar (s.
    // Settings::inventory_transparency).
    SettingsInventory,
    // Schrift: Minecraft-Bitmapschrift oder DM Sans fuer ALLEN Text des Clients.
    SettingsFont,
    // Design: wie der Client aussieht -- bisher das Hintergrundbild des Startbildschirms
    // (s. wallpaper.rs).
    SettingsDesign,
    // Welche Sprache wo gilt (s. i18n, LANGUAGE_ROWS).
    SettingsLanguage,
    // Module: dieselbe Tafel wie das Modul-Menue im Spiel (s. build_module_menu), nur aus den
    // Einstellungen heraus erreichbar -- auch ohne Verbindung.
    SettingsModules,
    // Sens-Rechner (Knopf "Rechner" neben der Kamera-Empfindlichkeit): Minecraft-Sens in
    // unsere umrechnen, und cm/360 aus DPI und Sens. Unterscreen von SettingsControls.
    SettingsSensCalc,
    // Shadereinstellungen (Knopf neben dem Renderpfad im Render-Tab): Werte, die nur den
    // Shader-Pfad betreffen. Unterscreen von SettingsGeneral.
    SettingsShader,
    // Configs: zwischen den benannten Einstellungs-Dateien wechseln, neue anlegen, umbenennen,
    // loeschen -- wie im Launcher, aber im laufenden Spiel (s. App::switch_config).
    SettingsConfigs,
    // Lichtstaerken: je Lichtquelle ein Faktor, mit Suche. Unterscreen von SettingsShader.
    SettingsLightLevels,
    // Partikel-Anpassung: je Partikelart an/aus, Dauer, Deckkraft, Tempo. Unterscreen von
    // SettingsVideo (Knopf neben der Partikel-Stufe).
    SettingsParticles,
    // Animationen: je Animation aus world::more_anims::FEATURES ein Schalter. Unterscreen von
    // SettingsVideo (Knopf im Abschnitt Animationen).
    SettingsAnimations,
    // Sonstiges: was zu keiner anderen Seite passt (Server-Bildschirme, Chat leeren, ...).
    SettingsMisc,
    // Server-Bildschirme: je Bildschirm, ob der Server ihn schliessen und oeffnen darf (s.
    // Settings::server_close_blocked). Unterseite von SettingsMisc.
    SettingsServerScreens,
    // Einrichtung: ganz unten in der Kategorienleiste, erste Seite beim allerersten Oeffnen der
    // Einstellungen. Hier kommt der Tastenbelegungs-Assistent hin.
    SettingsSetup,
    // Suche: ganz unten in der Kategorienleiste, unter Setup. Enter sucht in allen
    // Einstellungsseiten und Modulen (s. App::run_settings_search).
    SettingsSearch,
    HudEditor,
    // Gestorben-Screen: erzwungen, wenn der Spieler tot ist (Leben <= 0). Kein Weg zurueck
    // ausser Respawn oder Disconnect -- Esc tut hier nichts.
    Death,
}

impl MenuScreen {
    // Zu welcher Kategorie der Einstellungen ein Screen gehoert: Kategorien zu sich selbst,
    // Unterseiten (Rechner, Shader, ...) zu der Kategorie, von der aus man sie oeffnet.
    // None = keine Einstellungsseite.
    pub(crate) fn settings_category(self) -> Option<Self> {
        match self {
            Self::SettingsSensCalc => Some(Self::SettingsControls),
            Self::SettingsShader | Self::SettingsLightLevels => Some(Self::SettingsGeneral),
            Self::SettingsParticles | Self::SettingsAnimations => Some(Self::SettingsVideo),
            Self::SettingsServerScreens => Some(Self::SettingsMisc),
            Self::SettingsSetup | Self::SettingsSearch => Some(self),
            s if SETTINGS_CATEGORIES.iter().any(|(c, _)| *c == s) => Some(s),
            _ => None,
        }
    }

    // Eine Kategorie selbst (keine Unterseite): dort ersetzt die Kategorienleiste den
    // Zurueck-Knopf.
    pub(crate) fn is_settings_category(self) -> bool {
        self.settings_category() == Some(self)
    }

    // Name in der Einstellungsdatei (settings_category=), unabhaengig von der Sprache.
    pub(crate) const fn settings_key(self) -> &'static str {
        match self {
            Self::SettingsGeneral => "render",
            Self::SettingsVideo => "video",
            Self::SettingsAudio => "audio",
            Self::SettingsControls => "controls",
            Self::SettingsInventory => "inventory",
            Self::SettingsSkin => "skin",
            Self::SettingsFont => "font",
            Self::SettingsDesign => "design",
            Self::SettingsLanguage => "language",
            Self::SettingsModules => "modules",
            Self::SettingsConfigs => "configs",
            Self::SettingsMisc => "misc",
            Self::SettingsSearch => "search",
            // Nur fuer ihren Scrollstand (App::page_scrolls), keine Kategorie.
            Self::SettingsAnimations => "animations",
            Self::SettingsServerScreens => "server_screens",
            _ => "setup",
        }
    }

    pub(crate) fn from_settings_key(key: &str) -> Option<Self> {
        SETTINGS_CATEGORIES.iter().map(|(c, _)| *c).chain([Self::SettingsSetup, Self::SettingsSearch]).find(|c| c.settings_key() == key)
    }

    // Seiten im gemeinsamen Zeilenraster mit Seiten-Scroll (s. SettingsPage). None: eigenes
    // Layout (Tabellen, Listen mit eigenem Scroll) oder noch nicht umgestellt.
    pub(crate) fn settings_page(self) -> Option<SettingsPage> {
        match self {
            Self::SettingsGeneral => Some(SettingsPage {
                rows: SETTINGS_ROW_COUNT,
                sections: &[("Bildrate", FPS_LIMIT_ROW), ("Chunks", MESH_BUDGET_ROW), ("Effekte", MOTION_BLUR_ROW), ("Tearing", TEARING_TEST_ROW), ("System", CPU_ROW)],
            }),
            Self::SettingsVideo => Some(SettingsPage {
                rows: VIDEO_ROW_COUNT,
                sections: &[("Sichtweite", RENDER_DISTANCE_ROW), ("Kamera", VIDEO_CAMERA_ROW), ("Darstellung", SMOOTH_LIGHTING_ROW), ("Treffer", DAMAGE_TINT_FIRST_ROW), ("Spielerliste", PLAYER_LIST_SCALE_ROW), ("Hurt-Cam", HURT_CAM_ROW), ("Schulterkamera", SHOULDER_ROW), ("Feuer", FIRE_MODE_ROW), ("Tod", DEATH_ANIM_ROW), ("Animationen", ANIM_ROW), ("Modified View Model", VIEW_MODEL_ROW)],
            }),
            Self::SettingsControls => Some(SettingsPage { rows: controls_rows().len(), sections: controls_sections() }),
            // Zeile 4 = Zusammenfassen bei Hover, Zeilen 5..7 = Trank-Buchstaben.
            Self::SettingsInventory => Some(SettingsPage { rows: INVENTORY_ROW_COUNT, sections: &[("Allgemein", 0), ("Tränke", 6), ("Animation", INVENTORY_ANIM_ROW)] }),
            Self::SettingsMisc => Some(SettingsPage { rows: 5, sections: &[] }),
            Self::SettingsAnimations => Some(SettingsPage { rows: world::more_anims::FEATURES.len(), sections: &[] }),
            Self::SettingsServerScreens => Some(SettingsPage { rows: SERVER_SCREENS_ROWS, sections: &[("Server schließt", 0), ("Server öffnet", SERVER_OPEN_ROW)] }),
            Self::SettingsDesign => Some(SettingsPage { rows: design_row_count(), sections: &[("Allgemein", 0), ("Theme", DESIGN_THEME_ROW)] }),
            Self::SettingsAudio => Some(SettingsPage { rows: AUDIO_AUTO_ROW + 1, sections: &[("Lautstärke", AUDIO_MASTER_ROW), ("UI-Klänge", AUDIO_UI_SOUND_FIRST_ROW), ("Ausgabe", AUDIO_DEVICE_ROW)] }),
            // Aussehen (Groesse, Breite, Hintergrund, Abdunkeln), dann Verlauf (Zeilen, Ausblenden).
            Self::SettingsFont => Some(SettingsPage { rows: CAPITALS_ROW + 1, sections: &[("Standard", 0), ("Ausnahmen", FONT_EXCEPTIONS_ROW), ("Zahlen", STABLE_NUMBERS_ROW), ("Stil", CAPITALS_ROW)] }),
            Self::SettingsLanguage => Some(SettingsPage { rows: LOANWORDS_ROW + 1, sections: &[("Sprachen", 0), ("Schreibweise", LATIN_MACRONS_ROW)] }),
            // Suchfeld, dann je Treffer eine Zeile (oder die Zeile "nichts gefunden").
            Self::SettingsSearch => Some(SettingsPage { rows: 2 + SEARCH_HIT_COUNT.load(std::sync::atomic::Ordering::Relaxed), sections: &[] }),
            _ => None,
        }
    }
}

// Ansicht waehrend Freelook (Settings::freelook_view): 0 = die gerade eingestellte, sonst fest
// eine der drei.
pub(crate) const FREELOOK_VIEW_FIRST: u8 = 1;
pub(crate) const FREELOOK_VIEW_BACK: u8 = 2;
pub(crate) const FREELOOK_VIEW_FRONT: u8 = 3;
pub(crate) const FREELOOK_VIEW_NAMES: [&str; 4] = ["Aktuelle", "Ich-Sicht", "Hinten", "Vorne"];

// So viele Pixel steht ein angesprungener Abschnitt tiefer als die erste Zeile (s.
// SettingsPage::section_stand). Hoechstens 8: tiefer kaeme seine eigene Trennlinie samt Name
// unter der Reiterlinie hervor (s. App::build_settings_page_frame).
pub(crate) const SECTION_TOP_MARGIN: f32 = 7.0;

// Wie lange eine Zeile nach einem Klick auf ihren Suchtreffer aufleuchtet.
pub(crate) const SEARCH_FLASH_SECS: f32 = 1.6;

// Treffer der letzten Suche (s. App::search_hits): fuer die Zeilenzahl der Suchseite, die
// MenuScreen::settings_page ohne App kennen muss.
pub(crate) static SEARCH_HIT_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

// Ein Treffer der Einstellungssuche: Beschriftung der Zeile, wo sie steht (fuer die Anzeige,
// z.B. "Video › Darstellung") und wohin ein Klick fuehrt.
#[derive(Clone)]
pub(crate) struct SearchHit {
    pub(crate) label: String,
    pub(crate) path: String,
    pub(crate) place: SearchPlace,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SearchPlace {
    // Einstellungsseite und Zeile.
    Page(MenuScreen, usize),
    // Modul und Detailzeile (0 = Hauptschalter).
    Module(ModuleId, usize),
}

// Eine Einstellungsseite mit Seiten-Scroll: wie viele Zeilen sie hat und wo ihre Abschnitte
// beginnen (Name, erste Zeile). Ab zwei Abschnitten stehen oben Reiter, die sofort zum
// Abschnitt scrollen, und vor jedem weiteren Abschnitt eine Trennlinie mit seinem Namen rechts
// (s. App::build_settings_page_frame). Die Seite selbst setzt ihre Zeilen wie immer ueber
// settings_row_y; den Platz fuer die Trennlinien rechnet das dort hinein.
#[derive(Clone, Copy)]
pub(crate) struct SettingsPage {
    pub(crate) rows: usize,
    pub(crate) sections: &'static [(&'static str, usize)],
}

impl SettingsPage {
    // Bitmaske fuer settings_row_y_in. Der erste Abschnitt bekommt keine Trennlinie -- direkt
    // darueber stehen die Reiter.
    pub(crate) fn section_starts(self) -> u128 {
        self.sections.iter().filter(|&&(_, row)| row > 0 && row < 128).fold(0, |m, &(_, row)| m | 1 << row)
    }

    // Scrollstand (in Zeilen), bei dem die Zeile `row` oben steht -- ein Abschnittsanfang um
    // SECTION_TOP_MARGIN tiefer, damit sein Aufleuchten (s. App::push_section_flash) unter der
    // Linie unter den Reitern bleibt, statt sie zu verdecken.
    pub(crate) fn section_stand(self, row: usize) -> f32 {
        if row == 0 {
            return 0.0;
        }
        let starts = self.section_starts();
        let (a, b) = (settings_row_pos(row as f32 - 1.0, starts), settings_row_pos(row as f32, starts));
        row as f32 - 1.0 + ((b - SECTION_TOP_MARGIN - a) / (b - a)).clamp(0.0, 1.0)
    }
}

// Verschiebbare HUD-Elemente im HudEditor (s. hud_drag_offset). Weitere Elemente (Leben,
// Hunger, Hotbar) bekommen spaeter eigene Varianten nach demselben Muster.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HudElement {
    Perf,
    Crosshair,
    // Server-HUD, jetzt ebenfalls verschiebbar (s. build_server_hud). Positionen normiert wie
    // Perf (obere linke Ecke), Groesse aus dem tatsaechlichen Inhalt bzw. Platzhalter im Editor.
    ActionBar,
    Scoreboard,
    // Die Hotbar selbst (Sheet + Icons + Auswahlrahmen) -- verschiebbar wie alles andere.
    Hotbar,
    // Der Chat-Nachrichtenblock. Die Eingabezeile gehoert bewusst NICHT dazu: die bleibt am
    // unteren Bildschirmrand, wo eine Texteingabe hingehoert.
    Chat,
    // Die eigene Schild-Cooldown-Zahl (Schild-Modul), gezeichnet in Fadenkreuzfarbe.
    ShieldCooldown,
    // Die Munitionsanzeige des Pfeil-Moduls.
    Arrows,
    // Titel und Untertitel (Title-Pakete), je fuer sich verschiebbar, ausrichtbar und
    // skalierbar.
    Title,
    Subtitle,
    // Die Statuseffekte oben rechts (Gui.extractEffects), verschiebbar und skalierbar; dazu
    // die Schalter fuer die Anzeige im HUD und neben dem Inventar. Die Spielerliste steht
    // bewusst NICHT im Editor -- sie sitzt fest oben mittig wie in Vanilla.
    Effects,
    // Die Bossbars (BossBarHud), alle zusammen als ein Block.
    BossBar,
    // Der Name des gewaehlten Items ueber der Hotbar (s. build_held_name).
    HeldName,
    // Das Ruestungs-HUD (Modul "Ruestung", s. build_armor_hud).
    Armor,
    // Die Keystrokes (Modul, s. push_keystrokes).
    Keystrokes,
    // Die Session-Info (Modul, s. push_session_info).
    Session,
}

impl HudElement {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 16] = [
        Self::Perf,
        Self::Crosshair,
        Self::Title,
        Self::Subtitle,
        Self::ActionBar,
        Self::Scoreboard,
        Self::Effects,
        Self::Hotbar,
        Self::Chat,
        Self::ShieldCooldown,
        Self::Arrows,
        Self::BossBar,
        Self::HeldName,
        Self::Armor,
        Self::Keystrokes,
        Self::Session,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Perf => "Performance-Anzeige",
            Self::Crosshair => "Fadenkreuz",
            Self::ActionBar => "Actionbar",
            Self::Scoreboard => "Scoreboard",
            Self::Hotbar => "Hotbar",
            Self::Chat => "Chat",
            Self::ShieldCooldown => "Schild-Cooldown",
            Self::Arrows => "Pfeil-Anzeige",
            Self::Title => "Titel",
            Self::Subtitle => "Untertitel",
            Self::Effects => "Effekte",
            Self::BossBar => "Bossbar",
            Self::HeldName => "Itemname",
            Self::Armor => "Rüstung",
            Self::Keystrokes => "Keystrokes",
            Self::Session => "Session-Info",
        }
    }
}

// Eingabe, die bei jeder Obergrenze "unbegrenzt" bedeutet (s. EditableValue::
// allows_unlimited). Ein Wert statt eines Worts, damit man ihn auf jeder Tastaturbelegung
// tippen kann und keine Uebersetzung raten muss.
pub(crate) const UNLIMITED_INPUT: &str = "-1";

// Numerische Einstellungen, deren Wertfeld sich anklicken laesst, um statt der Pfeiltasten
// eine eigene Zahl per Freitext einzugeben (s. App::editing_value/begin_edit_value) --
// zusaetzlich zum bestehenden Stepper, nicht als Ersatz.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditableValue {
    MouseSensitivity,
    MouseSensitivityY,
    CursorSensitivity,
    MeshBudget,
    Fov(Perspective),
    CameraDistance(Perspective),
    CameraYOffset(Perspective),
    FpsLimit,
    MaxRenderDistance,
    InventoryTransparency,
    GlintSpeed,
    GlintStrength,
    PlayerListScale,
    MenuBackdropOpacity,
    ScrollInertia,
    ScrollDistance,
    EntityRenderDistance,
    BlockEntityRenderDistance,
    // Sens-Rechner (s. MenuScreen::SettingsSensCalc).
    CalcMinecraftSens,
    CalcDpi,
    // Lautstaerken im Audio-Tab (Haupt- und je Kategorie, s. VOLUME_CATEGORIES).
    MasterVolume,
    CategoryVolume(usize),
    // Lautstaerke eines einzelnen UI-Klangs (Index in ui_sound::UiSound::ALL).
    UiSoundVolume(usize),
    // Staerke neben Schatten-Kaestchen `i` im Schrift-Tab (0 = Standard, 1 + Bereich): die
    // Dicke, wenn darin die Kontur liegt, sonst die Deckkraft. Beide liegen an derselben
    // Stelle, sichtbar ist nur der passende (s. App::editable_shown).
    ShadowThickness(usize),
    ShadowOpacity(usize),
}

// Eine einstellbare Farbe, die das Farbfenster (s. ColorPopup) bearbeitet. Jede kennt ihre
// Achsen (Ton, Saettigung, Helligkeit, ggf. Deckkraft) -- s. App::color_hsva/set_color_hsva.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColorTarget {
    Theme(theme::Slot),
    Hitbox(HitboxVariant),
    HitboxKind(hitbox_kinds::HitboxKind),
    // Feste Farbe einer Seite im Teams-Modul.
    Team(teams::Side),
    Shield(ShieldColor),
    Sky,
    BlockOutline,
    DamageTint,
    Crit,
    Crosshair,
    SaturationHud,
    // Stueckzahl im Slot: true = Hotbar, false = Inventar.
    StackCount(bool),
    // Hervorhebung des Slots unter dem Zeiger (Settings::slot_hover_color).
    SlotHover,
    // Modul Eigenname: 0, 1 = Farben des Verlaufs, 2, 3 = Schatten.
    EgoName(u8),
    // Fadenkreuzfarbe beim Brennen (Feuer-Modus "Fadenkreuz").
    FireCrosshair,
    // Farb-Einstellung eines Addon-Moduls (Modul, Einstellung).
    Addon(u16, u16),
}

// Das offene Farbfenster: wofuer, unter welchem Farbfeld (Bildschirmkoordinaten), der Stand
// als Gleitkommazahlen (Ton 0..360, Rest 0..100) -- damit Ton und Saettigung nicht verloren
// gehen, wenn man die Helligkeit kurz auf 0 zieht -- und welcher Regler gerade gezogen wird.
pub(crate) struct ColorPopup {
    pub(crate) target: ColorTarget,
    pub(crate) anchor: Rect,
    pub(crate) hsva: [f32; 4],
    pub(crate) drag: Option<usize>,
}

// Eine Auswahl aus festen, benannten Werten, die als Aufklappliste (wie in osu!lazer) statt
// als Durchschalt-Knopf erscheint -- ab drei Werten; zwei sind zwei Knoepfe nebeneinander.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Projection,
    // Partikel-Stufe: true = die eigenen Partikel.
    Particles(bool),
    Wallpaper,
    // Zeile im Sprach-Tab (0 = alles zusammen).
    Language(usize),
    HoldUseRepeat,
    ArrowEmptyFeedback,
    SkyMode,
    WeatherMode,
    DeathAnimation,
    EgoDeco,
    EgoShadow,
    EgoSound,
    AudioDevice,
    AudioBuffer,
    CrosshairShape,
    // Auswahl-Einstellung eines Addon-Moduls (Modul, Einstellung).
    Addon(u16, u16),
}

// Die offene Aufklappliste: welche Auswahl, unter welchem Knopf (Bildschirmkoordinaten), ab
// welchem Eintrag sie zeigt und wo sie geoeffnet wurde -- wechselt die Ansicht, gilt sie als zu.
pub(crate) struct Dropdown {
    pub(crate) choice: Choice,
    pub(crate) anchor: Rect,
    // Scrollstand in Eintraegen (s. tick_ui_anim).
    pub(crate) scroll: smooth_scroll::SmoothScroll,
    pub(crate) mode: AppMode,
    pub(crate) screen: MenuScreen,
    pub(crate) module_menu: bool,
}

// Eine Liste, die sich mit gedrueckter linker Taste ziehen laesst (s. smooth_scroll::DragGrip).
// Die Einstellungsseite merkt sich ihren Screen: wechselt er waehrend des Ziehens, gilt das
// Ziehen als vorbei.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ScrollList {
    SettingsPage(MenuScreen),
    Particles,
    LightLevels,
    ModuleMenu,
    Dropdown,
    Dialog,
    CodeOfConduct,
}

pub(crate) const DROPDOWN_ITEM_H: f32 = 36.0;
pub(crate) const DROPDOWN_MAX_ITEMS: usize = 8;
// Innenabstand zum Rahmen. Oben und unten liegt darin die Linie am Listenende (s.
// build_dropdown) -- sie soll nicht in den ersten Eintrag ragen.
pub(crate) const DROPDOWN_PAD: f32 = 8.0;
// Bildlaufleiste rechts, breit wie in osu!lazer; die Eintraege ruecken dafuer zur Seite.
pub(crate) const DROPDOWN_BAR_W: f32 = 8.0;

// Wie schnell eine Liste dem Mausrad nachgleitet, als Rate je Sekunde fuer
// `1 - exp(-dt * rate)`: 0 % springt sofort, 50 % ist etwa die Rate der Serverliste (16/s),
// 100 % gleitet traege mit 4/s aus.
pub(crate) fn scroll_inertia_rate(prozent: u32) -> f32 {
    if prozent == 0 {
        return f32::INFINITY;
    }
    60.0 * (4.0f32 / 60.0).powf(prozent.min(100) as f32 / 100.0)
}

// Plaetze je Seite des Server-Icon-Atlas (8x8 = 64 Server).
pub(crate) const SERVER_ICON_GRID: u32 = 8;
// Der letzte Platz gehoert dem Vanilla-Bild fuer Server ohne Icon (textures/misc/
// unknown_server.png, aus den Resourcepacks); die Server-Icons gehen reihum durch die anderen.
pub(crate) const UNKNOWN_SERVER_ICON_SLOT: usize = (SERVER_ICON_GRID * SERVER_ICON_GRID) as usize - 1;

// Ladekreis: Punkte im Kreis um die Mitte von `feld`, der hellste laeuft herum und zieht
// verblassende hinter sich her. `t` = Sekunden seit Beginn des Ladens.
pub(crate) const LOADING_DOTS: usize = 8;
pub(crate) const LOADING_TURN_S: f32 = 0.8;

pub(crate) fn loading_dots(feld: Rect, t: f32, color: [f32; 4]) -> Vec<UiRect> {
    let (mx, my) = (feld.x + feld.w / 2.0, feld.y + feld.h / 2.0);
    let r = feld.w.min(feld.h) * 0.28;
    let d = (feld.w.min(feld.h) * 0.12).max(3.0);
    let kopf = (t / LOADING_TURN_S * LOADING_DOTS as f32) as usize % LOADING_DOTS;
    (0..LOADING_DOTS)
        .map(|i| {
            let w = i as f32 / LOADING_DOTS as f32 * std::f32::consts::TAU;
            let (px, py) = (mx + r * w.sin(), my - r * w.cos());
            // Abstand hinter dem Kopf: 0 = Kopf, voll deckend; die letzten fast weg.
            let hinten = (kopf + LOADING_DOTS - i) % LOADING_DOTS;
            let a = 1.0 - hinten as f32 / LOADING_DOTS as f32 * 0.85;
            let farbe = [color[0], color[1], color[2], color[3] * a];
            UiRect::rounded(Rect { x: px - d / 2.0, y: py - d / 2.0, w: d, h: d }, farbe, d / 2.0)
        })
        .collect()
}

// UV-Rechteck von Platz `slot` im Server-Icon-Atlas.
// UV-Rechteck von Platz `slot` im Pack-Atlas (s. GpuState::upload_pack_icon).
pub(crate) fn logo_uv(slot: usize) -> [f32; 4] {
    let n = LOGO_GRID as f32;
    let (x, y) = ((slot as u32 % LOGO_GRID) as f32, (slot as u32 / LOGO_GRID) as f32);
    [x / n, y / n, (x + 1.0) / n, (y + 1.0) / n]
}

pub(crate) fn pack_icon_uv(slot: usize) -> [f32; 4] {
    let n = skin_tab::ICON_GRID as f32;
    let (x, y) = ((slot as u32 % skin_tab::ICON_GRID) as f32, (slot as u32 / skin_tab::ICON_GRID) as f32);
    [x / n, y / n, (x + 1.0) / n, (y + 1.0) / n]
}

pub(crate) fn server_icon_uv(slot: usize) -> [f32; 4] {
    let n = SERVER_ICON_GRID as f32;
    let (x, y) = ((slot as u32 % SERVER_ICON_GRID) as f32, (slot as u32 / SERVER_ICON_GRID) as f32);
    [x / n, y / n, (x + 1.0) / n, (y + 1.0) / n]
}

// Masse des Farbfensters (Design-System: 288 breit, Vorschau 120 hoch, Spur 12).
pub(crate) const COLOR_POPUP_W: f32 = 288.0;
pub(crate) const COLOR_POPUP_PAD: f32 = 14.0;
pub(crate) const COLOR_POPUP_PREVIEW_H: f32 = 120.0;
pub(crate) const COLOR_POPUP_ROW_H: f32 = 44.0;
pub(crate) const COLOR_POPUP_TRACK_H: f32 = 12.0;
// Farbfeld in Einstellungszeilen: 54x36 mit schwarzem Rand.
pub(crate) const SWATCH_W: f32 = 54.0;

// Aussehen: Abschnitt Allgemein (Zeile 0 Spielbild abdunkeln, Zeile 1 Hintergrundbild), danach
// Theme: links die Kacheln aller Themes, rechts hinter einer Trennlinie die Farben des
// gewaehlten. Die Seite hat keinen Erklaerungsrand (s. side_strip_w), die Farben brauchen ihn.
// Abschnitt Allgemein: wie stark das Spielbild hinter den Menues abgedunkelt wird, das
// Hintergrundbild und darunter dessen Knopf "Bild waehlen" bzw. der Pfad.
pub(crate) const MENU_BACKDROP_ROW: usize = 0;
pub(crate) const WALLPAPER_ROW: usize = 1;
pub(crate) const SCROLL_INERTIA_ROW: usize = 2;
pub(crate) const SCROLL_DISTANCE_ROW: usize = 3;
// Rechts neben der Wahl: "Bild wählen..." bzw. der Pfad des Desktop-Bildes.
pub(crate) const WALLPAPER_PICK_X: f32 = SETTINGS_CTRL_X + 230.0;
pub(crate) const DESIGN_THEME_ROW: usize = 4;
// Trennlinie zwischen Kacheln und Farben.
pub(crate) const THEME_DIVIDER_X: f32 = 856.0;
pub(crate) const THEME_TILE_COLS: usize = 3;
pub(crate) const THEME_TILE_GAP: f32 = 12.0;
pub(crate) const THEME_TILE_W: f32 = ((THEME_DIVIDER_X - 16.0 - SETTINGS_LABEL_X - (THEME_TILE_COLS - 1) as f32 * THEME_TILE_GAP) / THEME_TILE_COLS as f32) as i32 as f32;
// Rechts: erste Zeile Name des Themes, darunter die Farbplaetze in zwei Spalten.
pub(crate) const THEME_COLORS_X: f32 = THEME_DIVIDER_X + 24.0;
pub(crate) const THEME_COLOR_COL_W: f32 = 400.0;
pub(crate) const THEME_COLOR_SWATCH_DX: f32 = 220.0;
pub(crate) const THEME_COLOR_ROWS: usize = theme::EDITABLE.len().div_ceil(2);
pub(crate) const THEME_NAME_MAX: usize = 24;
// Anzahl der Kacheln (Presets, eigene Themes, "+"), gesetzt von App::sync_settings_scroll --
// davon haengt ab, wie viele Zeilen die Seite hat (s. design_row_count).
pub(crate) static THEME_TILE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(theme::PRESETS.len() + 1);

pub(crate) fn design_row_count() -> usize {
    let kacheln = THEME_TILE_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    DESIGN_THEME_ROW + kacheln.div_ceil(THEME_TILE_COLS).max(1 + THEME_COLOR_ROWS)
}

// Kachel `i`: erst die Presets, dann die eigenen Themes, zuletzt "+".
pub(crate) fn theme_tile_rect(i: usize) -> Rect {
    let col = (i % THEME_TILE_COLS) as f32;
    let y = settings_row_y(DESIGN_THEME_ROW + i / THEME_TILE_COLS);
    Rect { x: SETTINGS_LABEL_X + col * (THEME_TILE_W + THEME_TILE_GAP), y: y - 3.0, w: THEME_TILE_W, h: SETTINGS_CTRL_H + 6.0 }
}

// (Zeile, x der Beschriftung) von Farbplatz `i`.
pub(crate) fn theme_slot_cell(i: usize) -> (usize, f32) {
    (DESIGN_THEME_ROW + 1 + i % THEME_COLOR_ROWS, THEME_COLORS_X + (i / THEME_COLOR_ROWS) as f32 * THEME_COLOR_COL_W)
}

// Gewaehltes Theme: ein Preset (Farben fest) oder eines der eigenen (Index in
// Settings::custom_themes, Farben aenderbar).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ThemeChoice {
    Preset(usize),
    Custom(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CustomTheme {
    pub(crate) name: String,
    pub(crate) colors: theme::Colors,
}

// Gespeichert als `custom_theme=RRGGBB,RRGGBB,...|Name` -- der Name zuletzt, damit er jedes
// Zeichen enthalten darf. Fehlende Farben (aeltere Dateien) kommen aus dem Standard, die
// Markierung folgt dann dem Akzent.
pub(crate) fn parse_custom_theme(value: &str) -> Option<CustomTheme> {
    let (farben, name) = value.split_once('|')?;
    let mut colors = theme::default_colors();
    let werte: Vec<u32> = farben.split(',').map_while(theme::parse_hex).collect();
    for (c, w) in colors.iter_mut().zip(&werte) {
        *c = *w;
    }
    if werte.len() <= theme::Slot::Selection.index() {
        colors[theme::Slot::Selection.index()] = colors[theme::Slot::Accent.index()];
    }
    if werte.len() <= theme::Slot::Accent2.index() {
        colors[theme::Slot::Accent2.index()] = theme::accent2_for(colors[theme::Slot::Accent.index()]);
    }
    if werte.len() <= theme::Slot::Accent2Hover.index() {
        theme::derive_accent2_tones(&mut colors);
    }
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(THEME_NAME_MAX).collect();
    (!name.is_empty()).then_some(CustomTheme { name, colors })
}

pub(crate) fn serialize_custom_theme(t: &CustomTheme) -> String {
    let farben: Vec<String> = t.colors.iter().map(|c| format!("{c:06X}")).collect();
    format!("{}|{}", farben.join(","), t.name)
}

// Fenster mit einem Namensfeld (wie Quick Connect): fuer ein neues Theme oder den Alias des
// Moduls Eigenname -- dieses auch im Spiel, aus dem Modul-Menue.
pub(crate) struct ThemeNameDialog {
    pub(crate) field: text_field::TextField,
    pub(crate) dragging: bool,
    pub(crate) last_press: Option<(Instant, (f32, f32))>,
    pub(crate) purpose: NameDialogPurpose,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameDialogPurpose {
    Theme,
    EgoAlias,
}

// Druck in ein einzeiliges Feld bei Zeichengrenze `i`: ein zweiter kurz danach an derselben
// Stelle nimmt das Wort, sonst Cursor dorthin (mit Shift bis dorthin markieren), und Ziehen
// markiert ab hier.
// Gibt zurueck, ob es ein Doppelklick war (Wortauswahl, s. App::selection_sounds).
pub(crate) fn field_press(field: &mut text_field::TextField, last_press: &mut Option<(Instant, (f32, f32))>, dragging: &mut bool, i: usize, shift: bool, at: (f32, f32)) -> bool {
    let jetzt = Instant::now();
    let doppelt = last_press.is_some_and(|(t, p)| jetzt.duration_since(t).as_millis() <= MULTI_CLICK_MS && (at.0 - p.0).abs() + (at.1 - p.1).abs() <= MULTI_CLICK_PX);
    if doppelt {
        field.select_word_at(i);
        *last_press = None;
    } else {
        field.press(i, shift);
        *dragging = true;
        *last_press = Some((jetzt, at));
    }
    doppelt
}

// Ein Schieberegler ueber einem alten Pfeil-Stepper: die Pfeil-Aktionen bleiben, wie sie sind
// (mit ihren eigenen Schritten und Grenzen), der Regler fuehrt sie beim Ziehen so oft aus, bis
// der Wert unter dem Zeiger liegt. `value_rect` ist das alte Wertfeld zwischen den Pfeilen --
// dort schreiben die Tabs ihren Wert hin, s. App::build_menu_ui.
#[derive(Clone, Copy)]
pub(crate) struct StepSlider {
    pub(crate) prev: ButtonAction,
    pub(crate) next: ButtonAction,
    pub(crate) value: f64,
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) value_rect: Rect,
    // Fuer wie viele Ziffern die Wertanzeige Platz hat (s. slider_box_w).
    pub(crate) digits: u8,
    // Laenge der Bahn: sonst SLIDER_TRACK_W, in dicht besetzten Tabellen kuerzer (Partikel).
    pub(crate) track_w: f32,
    // Rechtes Ende der Bahn ohne Rechtsklick; mit gehaltener rechter Taste reicht sie bis
    // `max` (wie SliderRange::soft). Sonst gleich `max`.
    pub(crate) soft: f64,
}

impl StepSlider {
    // Rechtes Ende der Bahn: gestreckt das harte, sonst das weiche -- nie unter dem Wert.
    pub(crate) const fn end(&self, extended: bool) -> f64 {
        if extended {
            self.max
        } else {
            self.soft.max(self.value).min(self.max)
        }
    }

    pub(crate) fn parts(&self) -> (Rect, Rect, Rect) {
        slider_parts_w(self.value_rect.x - SLIDER_GROW, self.value_rect.y, self.value_rect.h, self.digits, self.track_w)
    }

    pub(crate) fn rect(&self) -> Rect {
        self.parts().0
    }

    pub(crate) fn track(&self) -> Rect {
        self.parts().1
    }

    pub(crate) fn value_box(&self) -> Rect {
        self.parts().2
    }
}

// Welcher Regler gleitet (s. App::glide_slider).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SliderKey {
    Value(EditableValue),
    Step(ButtonAction),
    // Spur im Farbfenster, nach Position.
    Color(usize),
}

// Ganzer Regler um ein altes Wertfeld: er beginnt, wo der linke Pfeil stand.
pub(crate) fn slider_rect_of(value_rect: Rect, digits: u8) -> Rect {
    slider_parts(value_rect.x - SLIDER_GROW, value_rect.y, value_rect.h, digits).0
}

// Laenge der Bahn -- bei jedem Regler gleich, egal wie breit die Zahl daneben ist.
pub(crate) const SLIDER_TRACK_W: f32 = 345.0;
// Bahnlaenge der Rand-Regler in der Schrift-Tabelle (s. EditableValue::track_w).
pub(crate) const SHADOW_SLIDER_TRACK_W: f32 = 150.0;
// Luecke zwischen Bahn und Wertfeld.
pub(crate) const SLIDER_BOX_GAP: f32 = 12.0;
// Platz, den ein Regler mit dem breitesten Wertfeld (sechs Ziffern) ungefaehr braucht -- fuer
// Fenster, die rechts davon noch die Einheit zeigen muessen.
pub(crate) const SLIDER_SPAN_W: f32 = 6.0 + SLIDER_TRACK_W + SLIDER_BOX_GAP + 90.0;

// Ein Regler, der bei `x` beginnt: (ganzer Regler, Bahn, Wertfeld). Das Wertfeld steht rechts
// hinter der Bahn und ist so breit wie `digits` Ziffern (s. slider_box_w).
pub(crate) fn slider_parts(x: f32, y: f32, h: f32, digits: u8) -> (Rect, Rect, Rect) {
    slider_parts_w(x, y, h, digits, SLIDER_TRACK_W)
}

// Dasselbe mit eigener Bahnlaenge.
pub(crate) fn slider_parts_w(x: f32, y: f32, h: f32, digits: u8, track_w: f32) -> (Rect, Rect, Rect) {
    let bahn = Rect { x: x + 6.0, y: y + h / 2.0 - 2.0, w: track_w, h: 4.0 };
    let w = slider_box_w(digits);
    let kasten = Rect { x: bahn.x + bahn.w + SLIDER_BOX_GAP, y, w, h };
    (Rect { x, y, w: kasten.x + w - x, h }, bahn, kasten)
}

// Wertebereich eines Schiebereglers in Anzeige-Einheiten: `soft` ist das rechte Ende, mit dem
// er startet; ein Rechtsklick vergroessert es schrittweise bis `hard` (s.
// App::extend_slider). `decimals` = Nachkommastellen der Anzeige.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SliderRange {
    pub(crate) min: f64,
    pub(crate) soft: f64,
    pub(crate) hard: f64,
    pub(crate) decimals: usize,
}

// Wie weit der Regler die Pfeile links/rechts des Wertfelds mitbenutzt (die er ersetzt).
pub(crate) const SLIDER_GROW: f32 = STEPPER_BTN_W + STEPPER_GAP;

// Breite einer Ziffer bei UI_SCALE_SMALL in der aktuellen Schrift (f32-Bits), jedes Bild von
// build_menu_ui gesetzt. Die Wertanzeige eines Reglers ist so breit wie drei Ziffern (bei
// FPS, Entity- und LOD-Weite vier), die Einheit steht dahinter (s. push_slider_box).
pub(crate) static SLIDER_DIGIT_W: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x4140_0000);

pub(crate) fn slider_box_w(digits: u8) -> f32 {
    (f32::from_bits(SLIDER_DIGIT_W.load(std::sync::atomic::Ordering::Relaxed)) * f32::from(digits) + 16.0).round()
}

// Ein Reglerwert in Zahl und Einheit ("50 %" -> "50", "%"): die Zahl steht im Feld, die
// Einheit dahinter. Was nicht mit einer Zahl beginnt ("Monitor", "Aus"), bleibt ganz im Feld.
pub(crate) fn split_slider_value(text: &str) -> (&str, Option<&str>) {
    match text.split_once(' ') {
        Some((zahl, einheit))
            if zahl.chars().any(|c| c.is_ascii_digit()) && zahl.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '+' | '-')) =>
        {
            (zahl, Some(einheit))
        }
        _ => (text, None),
    }
}

// Die Zahl in einem Reglerfeld oder einer getippten Eingabe: "12,5 %" -> 12.5, "x2.0" -> 2.0,
// eine Uhrzeit "18:30" -> 18.5 Stunden (dann zweiter Wert true). Woerter ("Aus", "Monitor",
// "unbegrenzt") haben keine.
pub(crate) fn slider_number(text: &str) -> Option<(f64, bool)> {
    let t = text.trim().replace(',', ".");
    let start = t.find(|c: char| c.is_ascii_digit() || matches!(c, '-' | '.'))?;
    let lauf: String = t[start..].chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ':' | '-')).collect();
    if let Some((h, m)) = lauf.split_once(':') {
        let h: f64 = h.parse().ok()?;
        let m: f64 = if m.is_empty() { 0.0 } else { m.parse().ok()? };
        return Some((h + m / 60.0, true));
    }
    lauf.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| (v, false))
}

// Uhrzeit-Regler (s. sky_time_label) beginnen um 06:00, dem Zeitpunkt 0 des Spieltags -- fuer
// den Vergleich zaehlt eine Uhrzeit deshalb in Stunden ab dort, sonst liefe die Anzeige nach
// 23:59 zurueck auf 00:00.
pub(crate) fn clock_from_day_start(stunden: f64) -> f64 {
    (stunden - 6.0).rem_euclid(24.0)
}

// Wertanzeige eines Reglers: die Zahl fuer das Feld, die Einheit als Text dahinter.
// "unbegrenzt" wird zum Unendlich-Zeichen, sofern die Schrift es hat. Passt die Zahl (oder ein
// Wort wie "Monitor") nicht, waechst das Feld nach rechts -- nach links ragte es in die Bahn.
// Rueckgabe: Feld und Feldtext.
pub(crate) fn push_slider_unit(gpu: &GpuState, kasten: Rect, wert: &str, texts: &mut Vec<UiText>) -> (Rect, String) {
    let (zahl, einheit) = split_slider_value(wert);
    let zahl = if zahl == "unbegrenzt" && gpu.advance('\u{221E}') > 0.0 { "\u{221E}".to_string() } else { zahl.to_string() };
    let feld = Rect { w: kasten.w.max((gpu.measure_text(&zahl, UI_SCALE_SMALL) + 16.0).round()), ..kasten };
    if let Some(einheit) = einheit {
        let y = (feld.y + (feld.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: einheit.to_string(), x: feld.x + feld.w + 8.0, y, color: theme::text_2(), scale: UI_SCALE_SMALL });
    }
    (feld, zahl)
}

impl EditableValue {
    // Bewusst NICHT frei eintippbar sind Einstellungen, deren Wertebereich so klein ist,
    // dass die Pfeile schneller sind als das Tippen, bzw. deren Wert an der Hardware haengt
    // statt am Geschmack: Chunk-Update-Threads (1..32, an der Kernzahl bemessen),
    // Render-Ahead (0..4), sowie alles, was gar keine Zahl ist (Projektion,
    // Fadenkreuz-Farbe, Audiogeraet).
    pub(crate) const ALL: [Self; 70] = [
        Self::MouseSensitivity,
        Self::MouseSensitivityY,
        Self::CursorSensitivity,
        Self::MeshBudget,
        Self::Fov(Perspective::FirstPerson),
        Self::Fov(Perspective::ThirdPersonBack),
        Self::Fov(Perspective::ThirdPersonFront),
        Self::CameraDistance(Perspective::ThirdPersonBack),
        Self::CameraDistance(Perspective::ThirdPersonFront),
        Self::CameraYOffset(Perspective::ThirdPersonBack),
        Self::CameraYOffset(Perspective::ThirdPersonFront),
        Self::FpsLimit,
        Self::MaxRenderDistance,
        Self::InventoryTransparency,
        Self::GlintSpeed,
        Self::GlintStrength,
        Self::PlayerListScale,
        Self::MenuBackdropOpacity,
        Self::ScrollInertia,
        Self::ScrollDistance,
        Self::EntityRenderDistance,
        Self::BlockEntityRenderDistance,
        Self::CalcMinecraftSens,
        Self::CalcDpi,
        Self::MasterVolume,
        Self::CategoryVolume(0),
        Self::CategoryVolume(1),
        Self::CategoryVolume(2),
        Self::CategoryVolume(3),
        Self::CategoryVolume(4),
        Self::CategoryVolume(5),
        Self::CategoryVolume(6),
        Self::CategoryVolume(7),
        Self::CategoryVolume(8),
        Self::CategoryVolume(9),
        Self::UiSoundVolume(0),
        Self::UiSoundVolume(1),
        Self::UiSoundVolume(2),
        Self::UiSoundVolume(3),
        Self::UiSoundVolume(4),
        Self::UiSoundVolume(5),
        Self::UiSoundVolume(6),
        Self::UiSoundVolume(7),
        Self::UiSoundVolume(8),
        Self::UiSoundVolume(9),
        Self::UiSoundVolume(10),
        Self::UiSoundVolume(11),
        Self::UiSoundVolume(12),
        Self::ShadowThickness(0),
        Self::ShadowOpacity(0),
        Self::ShadowThickness(1),
        Self::ShadowOpacity(1),
        Self::ShadowThickness(2),
        Self::ShadowOpacity(2),
        Self::ShadowThickness(3),
        Self::ShadowOpacity(3),
        Self::ShadowThickness(4),
        Self::ShadowOpacity(4),
        Self::ShadowThickness(5),
        Self::ShadowOpacity(5),
        Self::ShadowThickness(6),
        Self::ShadowOpacity(6),
        Self::ShadowThickness(7),
        Self::ShadowOpacity(7),
        Self::ShadowThickness(8),
        Self::ShadowOpacity(8),
        Self::ShadowThickness(9),
        Self::ShadowOpacity(9),
        Self::ShadowThickness(10),
        Self::ShadowOpacity(10),
    ];

    // Als Schieberegler gezeichnet? Die FPS-Grenze nicht: ihre Sonderwerte (Monitor,
    // unbegrenzt) liegen nicht auf einer Strecke.
    pub(crate) fn slider(self) -> Option<SliderRange> {
        let r = |min: f64, soft: f64, hard: f64, decimals: usize| Some(SliderRange { min, soft, hard, decimals });
        match self {
            // 0 = dem Monitor folgen (linkes Ende); unbegrenzt per "-1" eintippen.
            Self::FpsLimit => r(0.0, 360.0, f64::from(FPS_LIMIT_TYPED_MAX), 0),
            Self::MouseSensitivity | Self::MouseSensitivityY => {
                r(f64::from(SENSITIVITY_MIN) * 1000.0, 5.0, f64::from(SENSITIVITY_MAX) * 1000.0, 2)
            }
            Self::CursorSensitivity => r(f64::from(CURSOR_SENSITIVITY_MIN), 2.0, f64::from(CURSOR_SENSITIVITY_MAX), 2),
            Self::MeshBudget => r(1.0, 16.0, MESH_COLUMNS_MAX as f64, 0),
            Self::Fov(_) => r(f64::from(FOV_MIN), 110.0, f64::from(FOV_MAX), 0),
            Self::CameraDistance(_) => r(f64::from(CAMERA_DISTANCE_MIN), 8.0, f64::from(CAMERA_DISTANCE_MAX), 1),
            Self::CameraYOffset(_) => r(f64::from(CAMERA_Y_OFFSET_MIN), f64::from(CAMERA_Y_OFFSET_MAX), f64::from(CAMERA_Y_OFFSET_MAX), 1),
            Self::MaxRenderDistance => {
                r(f64::from(MAX_RENDER_DISTANCE_MIN), (f64::from(MAX_RENDER_DISTANCE_MAX) / 2.0).max(8.0).min(f64::from(MAX_RENDER_DISTANCE_MAX)), f64::from(MAX_RENDER_DISTANCE_MAX), 0)
            }
            Self::EntityRenderDistance => r(f64::from(ENTITY_DISTANCE_MIN), 128.0, f64::from(ENTITY_DISTANCE_MAX), 0),
            Self::BlockEntityRenderDistance => r(f64::from(BLOCK_ENTITY_DISTANCE_MIN), 128.0, f64::from(BLOCK_ENTITY_DISTANCE_MAX), 0),
            Self::ScrollDistance => r(f64::from(SCROLL_DISTANCE_MIN), f64::from(SCROLL_DISTANCE_MAX), f64::from(SCROLL_DISTANCE_MAX), 0),
            Self::CalcMinecraftSens => r(0.0, 200.0, 200.0, 0),
            Self::CalcDpi => r(f64::from(SENS_CALC_DPI_MIN), 3200.0, f64::from(SENS_CALC_DPI_MAX), 0),
            Self::PlayerListScale => r(f64::from(HUD_SCALE_PCT_MIN), f64::from(HUD_SCALE_PCT_MAX), f64::from(HUD_SCALE_PCT_MAX), 0),
            Self::InventoryTransparency
            | Self::GlintSpeed
            | Self::GlintStrength
            | Self::MenuBackdropOpacity
            | Self::ScrollInertia
            | Self::MasterVolume
            | Self::CategoryVolume(_)
            | Self::UiSoundVolume(_)
            | Self::ShadowOpacity(_) => r(0.0, 100.0, 100.0, 0),
            Self::ShadowThickness(_) => {
                let (min, max) = SHADOW_THICKNESS_RANGE;
                r(f64::from(min) / 10.0, f64::from(max) / 10.0, f64::from(max) / 10.0, 1)
            }
        }
    }

    // Ganzer Regler: beginnt, wo der linke Pfeil stand (s. slider_parts).
    pub(crate) fn slider_rect(self) -> Rect {
        let r = self.rect();
        slider_parts_w(r.x - SLIDER_GROW, r.y, r.h, self.box_digits(), self.track_w()).0
    }

    // Wertanzeige rechts im Regler -- dort tippt man eine Zahl ein (s. editable_value_at).
    pub(crate) fn slider_value_rect(self) -> Rect {
        let r = self.rect();
        slider_parts_w(r.x - SLIDER_GROW, r.y, r.h, self.box_digits(), self.track_w()).2
    }

    // Die Bahn, auf der der Daumen laeuft.
    pub(crate) fn slider_track(self) -> Rect {
        let r = self.rect();
        slider_parts_w(r.x - SLIDER_GROW, r.y, r.h, self.box_digits(), self.track_w()).1
    }

    // Laenge der Bahn: in der Schrift-Tabelle kuerzer, dort stehen Schrift und Rand schon
    // davor, und rechts am Rand der Seite die Namen der Abschnitte.
    pub(crate) const fn track_w(self) -> f32 {
        match self {
            Self::ShadowThickness(_) | Self::ShadowOpacity(_) => SHADOW_SLIDER_TRACK_W,
            _ => SLIDER_TRACK_W,
        }
    }

    // Vierstellige Werte bekommen eine breitere Anzeige, die Empfindlichkeiten mit bis zu vier
    // Nachkommastellen ("1.1222") eine noch breitere -- sonst ragte die Bahn in die Zahl.
    pub(crate) const fn box_digits(self) -> u8 {
        match self {
            Self::MouseSensitivity | Self::MouseSensitivityY | Self::CursorSensitivity => 6,
            Self::FpsLimit | Self::EntityRenderDistance | Self::BlockEntityRenderDistance | Self::CalcDpi => 4,
            _ => 3,
        }
    }

    // Auf welchem Screen das Feld liegt -- zusammen mit rect() die einzige Stelle, die die
    // Position kennt.
    pub(crate) const fn screen(self) -> MenuScreen {
        match self {
            Self::MouseSensitivity | Self::MouseSensitivityY | Self::CursorSensitivity => MenuScreen::SettingsControls,
            Self::MeshBudget | Self::FpsLimit => MenuScreen::SettingsGeneral,
            Self::Fov(_)
            | Self::CameraDistance(_)
            | Self::CameraYOffset(_)
            | Self::MaxRenderDistance
            | Self::EntityRenderDistance
            | Self::BlockEntityRenderDistance => MenuScreen::SettingsVideo,
            Self::InventoryTransparency => MenuScreen::SettingsInventory,
            Self::GlintSpeed | Self::GlintStrength | Self::PlayerListScale => MenuScreen::SettingsVideo,
            Self::MenuBackdropOpacity | Self::ScrollInertia | Self::ScrollDistance => MenuScreen::SettingsDesign,
            Self::CalcMinecraftSens | Self::CalcDpi => MenuScreen::SettingsSensCalc,
            Self::MasterVolume | Self::CategoryVolume(_) | Self::UiSoundVolume(_) => MenuScreen::SettingsAudio,
            Self::ShadowThickness(_) | Self::ShadowOpacity(_) => MenuScreen::SettingsFont,
        }
    }

    // Kommastellen nur, wo der Wert wirklich gebrochen ist. Maus-Empfindlichkeit und FOV
    // ja; Zaehlwerte (Chunks, Spalten, Hz) sind ganzzahlig, ein Punkt darin waere unsinnig.
    pub(crate) const fn allows_decimal(self) -> bool {
        matches!(
            self,
            Self::MouseSensitivity
                | Self::MouseSensitivityY
                | Self::CursorSensitivity
                | Self::CalcMinecraftSens
                | Self::Fov(_)
                | Self::CameraDistance(_)
                | Self::CameraYOffset(_)
                | Self::ShadowThickness(_)
                | Self::MasterVolume
                | Self::CategoryVolume(_)
                | Self::UiSoundVolume(_)
        )
    }

    // Ob "-1" hier "unbegrenzt" bedeutet. Nur bei echten OBERGRENZEN, deren Maximum
    // ohnehin schon "greift nie" heisst -- nicht bei FOV oder Empfindlichkeit, wo es
    // schlicht keinen unendlichen Wert gibt (der Bereich ist die Einstellung, kein Deckel).
    // Ob ein fuehrendes Minus ueberhaupt eingetippt werden darf. Das ist NICHT dasselbe wie
    // allows_unlimited: die Kamerahoehe nimmt echte negative Werte (Kamera unter Augenhoehe),
    // "-1" heisst dort schlicht "einen Block tiefer" und nicht "unbegrenzt".
    pub(crate) const fn allows_negative(self) -> bool {
        self.allows_unlimited() || matches!(self, Self::CameraYOffset(_))
    }

    pub(crate) const fn allows_unlimited(self) -> bool {
        matches!(self, Self::FpsLimit | Self::MaxRenderDistance | Self::EntityRenderDistance | Self::BlockEntityRenderDistance)
    }

    // Bildschirm-Rechteck des Wertfelds -- dieselbe Position, die auch die normale Anzeige
    // nutzt (stepper_value_rect), damit Klick-Trefferflaeche und Zeichnung nie auseinander-
    // laufen.
    pub(crate) fn rect(self) -> Rect {
        match self {
            Self::MouseSensitivity => stepper_value_rect(controls_row(ControlsRow::SensX)),
            Self::MouseSensitivityY => stepper_value_rect(controls_row(ControlsRow::SensY)),
            Self::CursorSensitivity => stepper_value_rect(controls_row(ControlsRow::CursorSens)),
            Self::FpsLimit => stepper_value_rect(FPS_LIMIT_ROW),
            Self::MeshBudget => stepper_value_rect(MESH_BUDGET_ROW),
            Self::MaxRenderDistance => stepper_value_rect(RENDER_DISTANCE_ROW), // SettingsVideo
            Self::Fov(p) => stepper_value_rect(p.fov_row()), // SettingsVideo
            // Die Ego-Ansicht hat keine Abstandszeile -- EditableValue::ALL fuehrt sie
            // deshalb gar nicht erst auf, das unwrap_or ist nur der Vollstaendigkeit halber.
            // Rechte Spalte des Video-Tabs (s. Perspective::distance_row). Die Ego-Ansicht
            // hat keine dieser Zeilen -- EditableValue::ALL fuehrt sie deshalb gar nicht
            // erst auf, das unwrap_or ist nur der Vollstaendigkeit halber.
            Self::CameraDistance(p) => stepper_value_rect(p.distance_row().unwrap_or(0)),
            Self::CameraYOffset(p) => stepper_value_rect(p.y_offset_row().unwrap_or(0)),
            Self::InventoryTransparency => stepper_value_rect(0), // SettingsInventory, Zeile 0
            Self::GlintSpeed => stepper_value_rect(GLINT_SPEED_ROW), // SettingsVideo
            Self::GlintStrength => stepper_value_rect(GLINT_STRENGTH_ROW),
            Self::PlayerListScale => stepper_value_rect(PLAYER_LIST_SCALE_ROW),
            Self::MenuBackdropOpacity => stepper_value_rect(MENU_BACKDROP_ROW),
            Self::ScrollInertia => stepper_value_rect(SCROLL_INERTIA_ROW),
            Self::ScrollDistance => stepper_value_rect(SCROLL_DISTANCE_ROW),
            Self::EntityRenderDistance => stepper_value_rect(ENTITY_DISTANCE_ROW),
            Self::BlockEntityRenderDistance => stepper_value_rect(BLOCK_ENTITY_DISTANCE_ROW),
            Self::CalcMinecraftSens => stepper_value_rect(SENS_CALC_MC_ROW),
            Self::CalcDpi => stepper_value_rect(SENS_CALC_DPI_ROW),
            Self::MasterVolume => stepper_value_rect(AUDIO_MASTER_ROW),
            Self::CategoryVolume(i) => stepper_value_rect(AUDIO_CATEGORY_FIRST_ROW + i),
            Self::UiSoundVolume(i) => stepper_value_rect(AUDIO_UI_SOUND_FIRST_ROW + i),
            // Zeile des Kaestchens, das Wertfeld so, dass der Regler bei SHADOW_STRENGTH_X beginnt.
            Self::ShadowThickness(i) | Self::ShadowOpacity(i) => {
                let row = if i == 0 { 0 } else { FONT_EXCEPTIONS_ROW + i - 1 };
                Rect { x: SHADOW_STRENGTH_X + SLIDER_GROW, y: settings_row_y(row), w: STEPPER_VALUE_W, h: SETTINGS_CTRL_H }
            }
        }
    }
}

// Die Knoepfe "1 px" / "1/2 px" neben dem Regler des Schatten-Kaestchens `i` (0 = Standard,
// 1 + Bereich), nur bei Vanilla (s. Settings::shadow_half).
pub(crate) fn shadow_half_rect(i: usize) -> Rect {
    let r = EditableValue::ShadowOpacity(i).slider_value_rect();
    Rect { x: r.x + r.w + 12.0, y: r.y, w: SHADOW_HALF_W, h: r.h }
}
