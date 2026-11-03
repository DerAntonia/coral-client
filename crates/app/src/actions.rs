// App-Modi, Eingabearten, Atlas-Slots, Auth- und Verbindungsereignisse, Button-Aktionen.

use crate::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppMode {
    Menu,
    World,
}

// Zweck des aktiven Connect-Screen-Eingabefelds (s. App::input_field).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputKind {
    // Fertiges Minecraft-Access-Token (manueller Login ohne Microsoft-Flow).
    Token,
    // Microsoft-Refresh-Token: wird gegen frische Tokens getauscht wie beim gespeicherten Login
    // und bleibt danach gespeichert -- die Anmeldung haelt also ueber Neustarts.
    RefreshToken,
    // Name einer Config (Configs-Seite der Einstellungen).
    ConfigName(ConfigEdit),
}

// Was das Namensfeld der Configs-Seite tut, wenn man es bestaetigt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ConfigEdit {
    // Neue Config mit Standardwerten.
    New,
    // Kopie der aktiven Config.
    Duplicate,
    // Die aktive Config umbenennen.
    Rename,
}

// Hoechstens so viele Configs stehen auf der Configs-Seite (ohne Scrollen).
pub(crate) const MAX_VISIBLE_CONFIGS: usize = 10;
// Erste Zeile der Config-Liste; Zeile 0 traegt die Knoepfe, Zeile 1 das Namensfeld.
pub(crate) const CONFIG_LIST_FIRST_ROW: usize = 2;
pub(crate) const CONFIG_BUTTON_W: f32 = 150.0;
pub(crate) const CONFIG_BUTTON_GAP: f32 = 10.0;

pub(crate) fn config_button_rect(i: usize) -> Rect {
    Rect { x: SETTINGS_CTRL_X + i as f32 * (CONFIG_BUTTON_W + CONFIG_BUTTON_GAP), y: settings_row_y(0), w: CONFIG_BUTTON_W, h: SETTINGS_CTRL_H }
}

pub(crate) fn config_name_field_rect() -> Rect {
    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(1), w: 3.0 * CONFIG_BUTTON_W + 2.0 * CONFIG_BUTTON_GAP, h: SETTINGS_CTRL_H }
}

pub(crate) fn config_row_rect(i: usize) -> Rect {
    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(CONFIG_LIST_FIRST_ROW + i), w: 3.0 * CONFIG_BUTTON_W + 2.0 * CONFIG_BUTTON_GAP, h: SETTINGS_CTRL_H }
}

// Vergibt einen Atlas-Platz. Rueckgabe: der Platz und (falls verdraengt wurde) wessen Eintrag
// jetzt ins Leere zeigt -- den muss der Aufrufer aufraeumen. None heisst: es gibt ueberhaupt
// keine Plaetze (Atlas zu klein).
//
// Verdraengt wird NICHT stumpf der aelteste, sondern der aelteste, den man GERADE NICHT SIEHT
// (`sichtbar` entscheidet das). Vorher gewann schlicht die Belegungsreihenfolge -- und damit
// konnte ein Spieler, der weit weg der Tabellenliste beitrat, dem Gegner direkt vor der Nase
// den Skin wegnehmen. So sprang der Skin manchmal auf Default.
// Erst wenn wirklich JEDER Platzinhaber im Bild ist, muss der aelteste weichen; dann sind
// mehr Spieler in Sicht als der Atlas Plaetze hat, und irgendwer muss es treffen.
//
// Der Platz steht MIT in der Reihenfolge-Schlange. Die fruehere Fassung merkte sich nur die
// UUIDs und suchte den Platz des Verdraengten hinterher in der Skin-Tabelle nachschlagend
// heraus -- mit `unwrap_or(0)`, falls er dort schon fehlte. Dann haette der neue Spieler den
// Platz 0 eines FREMDEN, noch gueltigen Eintrags ueberschrieben und zwei Spieler haetten
// denselben Skin getragen.
pub(crate) fn claim_atlas_slot(
    order: &mut std::collections::VecDeque<([u8; 16], usize)>,
    capacity: usize,
    uuid: [u8; 16],
    current: Option<usize>,
    sichtbar: impl Fn(&[u8; 16]) -> bool,
) -> Option<(usize, Option<[u8; 16]>)> {
    claim_atlas_slot_near(order, capacity, uuid, current, |u| sichtbar(u).then_some(0.0))
}

// Wie claim_atlas_slot, aber mit Abstand: None = gerade nicht gebraucht, sonst wie weit weg.
// Sind alle Platzinhaber gebraucht, weicht der am weitesten entfernte (bei Gleichstand der
// aelteste) -- nicht einfach der aelteste. Sonst verlor auf einem vollen Spawn ausgerechnet
// eine Statue aus Koepfen direkt vor einem ihre Skins, weil sie als Erstes geladen war.
pub(crate) fn claim_atlas_slot_near(
    order: &mut std::collections::VecDeque<([u8; 16], usize)>,
    capacity: usize,
    uuid: [u8; 16],
    current: Option<usize>,
    abstand: impl Fn(&[u8; 16]) -> Option<f32>,
) -> Option<(usize, Option<[u8; 16]>)> {
    if capacity == 0 {
        return None;
    }
    // Schon ein Platz fuer diesen Spieler? Dann denselben ueberschreiben.
    if let Some(slot) = current {
        return Some((slot, None));
    }
    if order.len() < capacity {
        let slot = order.len();
        order.push_back((uuid, slot));
        return Some((slot, None));
    }
    // Aeltester ungebrauchter Platzinhaber, sonst der entfernteste.
    let index = order.iter().position(|(u, _)| abstand(u).is_none()).unwrap_or_else(|| {
        let mut weitester = (0, f32::NEG_INFINITY);
        for (i, (u, _)) in order.iter().enumerate() {
            let d = abstand(u).unwrap_or(f32::INFINITY);
            if d > weitester.1 {
                weitester = (i, d);
            }
        }
        weitester.0
    });
    let (evicted, slot) = order.remove(index)?;
    order.push_back((uuid, slot));
    Some((slot, Some(evicted)))
}

// Unter welchem Schluessel der Skin eines Spielerkopfes in App::skins steht: die UUID aus der
// profile-Komponente, ohne sie ein Hash der Textur-Property (viele Server-Koepfe tragen nur
// die Textur). None, wenn der Kopf gar keinen Skin nennt.
pub(crate) fn head_skin_key(profil: &protocol::item::HeadProfile) -> Option<[u8; 16]> {
    world::entity::head_skin_key(profil)
}

// Schild mit Banner (ShieldModelRenderer): die Platte mit entity/shield_base statt
// shield_base_nopattern, darueber die Bannerlagen aus entity/shield/<muster>.
pub(crate) const SHIELD_BANNER_BASE: &str = "entity/shield_base";
// Alle Vanilla-Bannermuster (1.21.11), "base" ist die Grundfarben-Lage.
pub(crate) const SHIELD_PATTERNS: [&str; 43] = [
    "base", "border", "bricks", "circle", "creeper", "cross", "curly_border", "diagonal_left", "diagonal_right", "diagonal_up_left",
    "diagonal_up_right", "flow", "flower", "globe", "gradient", "gradient_up", "guster", "half_horizontal", "half_horizontal_bottom",
    "half_vertical", "half_vertical_right", "mojang", "piglin", "rhombus", "skull", "small_stripes", "square_bottom_left", "square_bottom_right",
    "square_top_left", "square_top_right", "straight_cross", "stripe_bottom", "stripe_center", "stripe_downleft", "stripe_downright",
    "stripe_left", "stripe_middle", "stripe_right", "stripe_top", "triangle_bottom", "triangle_top", "triangles_bottom", "triangles_top",
];

// DyeColor.getEntityColor je Farbindex (weiss, orange, magenta, ... schwarz).
pub(crate) const DYE_COLORS: [u32; 16] = [
    0xF9FFFE, 0xF9801D, 0xC74EBD, 0x3AB3DA, 0xFED83D, 0x80C71F, 0xF38BAA, 0x474F52, 0x9D9D97, 0x169C9C, 0x8932B8, 0x3C44AA, 0x835432, 0x5E7C16,
    0xB02E26, 0x1D1D21,
];

pub(crate) fn dye_rgb(index: u8) -> [f32; 3] {
    let c = DYE_COLORS[(index as usize).min(15)];
    [((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0]
}

// Seltenheit eines Stacks (0 common .. 3 epic) wie ItemStack.getRarity: die gesetzte
// rarity-Komponente, sonst die Vorgabe des Items; verzaubert steigt common/uncommon auf rare
// und rare auf epic. Die Vorgaben sind die aus Vanillas Items-Tabelle (1.21.11).
pub(crate) fn item_rarity(item: &str, stack: &protocol::item::ItemStack, verzaubert: bool) -> u8 {
    let vorgabe = match item {
        "enchanted_golden_apple" | "elytra" | "dragon_egg" | "command_block" | "chain_command_block" | "repeating_command_block"
        | "command_block_minecart" | "structure_block" | "structure_void" | "jigsaw" | "barrier" | "light" | "debug_stick" | "knowledge_book"
        | "heavy_core" | "mace" | "test_block" | "test_instance_block" => 3,
        "golden_apple" | "beacon" | "conduit" | "end_crystal" | "trident" | "nether_star" | "mojang_banner_pattern" | "music_disc_pigstep"
        | "music_disc_otherside" | "music_disc_5" | "music_disc_relic" | "music_disc_creator" | "music_disc_precipice" | "music_disc_tears"
        | "music_disc_lava_chicken" => 2,
        "experience_bottle" | "dragon_breath" | "heart_of_the_sea" | "totem_of_undying" | "enchanted_book" | "creeper_head" | "zombie_head"
        | "skeleton_skull" | "wither_skeleton_skull" | "player_head" | "dragon_head" | "piglin_head" | "trial_key" | "ominous_trial_key"
        | "ominous_bottle" | "sniffer_egg" | "creeper_banner_pattern" | "skull_banner_pattern" | "flow_banner_pattern" | "guster_banner_pattern"
        | "piglin_banner_pattern" | "globe_banner_pattern" | "netherite_upgrade_smithing_template" | "recovery_compass" | "disc_fragment_5"
        | "breeze_rod" | "wind_charge" => 1,
        n if n.starts_with("music_disc_") => 1,
        n if n.ends_with("_armor_trim_smithing_template") => 1,
        _ => 0,
    };
    let basis = stack.rarity.unwrap_or(vorgabe);
    match (verzaubert, basis) {
        (true, 0 | 1) => 2,
        (true, 2) => 3,
        _ => basis,
    }
}

// Trim-Materialien, Trim-Muster und Bannermuster in Registry-Reihenfolge -- Items nennen sie
// per Position (s. protocol::item::ItemStack::trim / banner_patterns).
#[derive(Default)]
pub(crate) struct TrimRegistries {
    // Je Material: Textur-Suffix ("amethyst") und die Sonderfaelle je Ruestungsmaterial
    // (override_armor_assets, z.B. Eisen-Trim auf Eisenruestung -> "iron_darker").
    pub(crate) materials: Vec<(String, std::collections::HashMap<String, String>)>,
    // Je Muster: Texturname ("sentry").
    pub(crate) patterns: Vec<String>,
    // Je Bannermuster: Texturname ("stripe_bottom").
    pub(crate) banners: Vec<String>,
    // Je Material der Registry-Name ("minecraft:quartz") -- fuer Item-Modelle (trim_material).
    pub(crate) material_ids: Vec<String>,
}

impl TrimRegistries {
    pub(crate) fn apply(&mut self, registry: &str, entries: Vec<(String, Option<protocol::nbt::NbtValue>)>) {
        use protocol::nbt::NbtValue;
        let ohne_ns = |s: &str| s.rsplit(':').next().unwrap_or(s).to_string();
        let feld = |d: &Option<NbtValue>, k: &str| d.as_ref().and_then(|d| d.get(k)).and_then(|v| v.as_str()).map(ohne_ns);
        match registry {
            "minecraft:trim_material" => {
                self.material_ids = entries.iter().map(|(name, _)| name.clone()).collect();
                self.materials = entries
                    .iter()
                    .map(|(name, d)| {
                        let mut ausnahmen = std::collections::HashMap::new();
                        if let Some(NbtValue::Compound(map)) = d.as_ref().and_then(|d| d.get("override_armor_assets")) {
                            for (k, v) in map {
                                if let Some(v) = v.as_str() {
                                    ausnahmen.insert(ohne_ns(k), ohne_ns(v));
                                }
                            }
                        }
                        (feld(d, "asset_name").unwrap_or_else(|| ohne_ns(name)), ausnahmen)
                    })
                    .collect();
            }
            "minecraft:trim_pattern" => self.patterns = entries.iter().map(|(name, d)| feld(d, "asset_id").unwrap_or_else(|| ohne_ns(name))).collect(),
            "minecraft:banner_pattern" => self.banners = entries.iter().map(|(name, d)| feld(d, "asset_id").unwrap_or_else(|| ohne_ns(name))).collect(),
            _ => {}
        }
    }
}

// Ein geladener (oder gerade ladender) Spieler-Skin.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlayerSkin {
    // Reservierter Platz im Entity-Atlas -- None, solange das Bild noch unterwegs ist.
    pub(crate) slot: Option<usize>,
    pub(crate) slim: bool,
    // Platz des Umhangs, falls das Konto einen hat und er geladen ist. Eigene Platzreihe im
    // Atlas (64x32 statt 64x64, s. assets::entity_atlas).
    pub(crate) cape_slot: Option<usize>,
}

// Nachrichten der Auth-Hintergrund-Tasks an den Hauptthread (s. App::auth_tx/rx). Der
// Device-Code-Flow laeuft asynchron: erst kommt der Code zum Anzeigen, spaeter (nachdem sich
// der Nutzer im Browser angemeldet hat) das fertige Konto oder ein Fehler.
pub(crate) enum AuthEvent {
    // Anzuzeigender Code + URL, unter der sich der Nutzer im Browser anmeldet.
    DeviceCode { user_code: String, verification_uri: String },
    // Browser-Login: diese Anmeldeseite (mit Kontoauswahl) oeffnen, s. auth::start_browser_login.
    BrowserLogin { url: String },
    // Anmeldung fertig: fertige Credentials, bei Microsoft das (neue) Refresh-Token mit der
    // Client-ID, fuer die es gilt, sonst die Quelle des Tokens (fuer die Kontenliste).
    LoggedIn { credentials: auth::Credentials, msa: Option<(String, String)>, source: accounts::Source },
    // Ein gespeichertes Konto ist geprueft (s. App::check_accounts): Schluessel und entweder
    // die frischen Credentials (bei Erneuerung mit neuem Refresh-Token) oder der Fehler.
    AccountChecked { key: String, result: Result<(auth::Credentials, Option<(String, String)>), auth::AuthError> },
    // Token-Export auskommentiert, bis Mojang die eigene App-ID freigibt.
    // // Token-Export (s. App::start_token_export): frische Credentials samt neuem Refresh-Token
    // // und Client-ID, oder der Fehler.
    // TokenExported { key: String, result: Result<(auth::Credentials, (String, String)), String> },
    // Fehlgeschlagen -- Meldung fuers Log.
    Failed(String),
    // Reiner Status-/Fortschrittstext fuers Log.
    Status(String),
    // Ein Spieler-Skin ist geladen (s. request_skin): UUID, dekodierte 64x64-RGBA-Pixel und
    // ob es das Slim-Modell ist. Geht ueber denselben Kanal wie die Anmeldung, weil es
    // dieselbe Bauform ist -- Hintergrund-Task, Ergebnis in den Hauptthread.
    //
    // Die URL kommt mit zurueck, damit der Hauptthread die Anfrage aus `requested_skins`
    // austragen kann: dieses Set ist die "laeuft gerade"-Sperre, keine Ewigkeitsliste
    // (s. App::requested_skins).
    SkinLoaded { uuid: [u8; 16], url: String, rgba: Vec<u8>, slim: bool },
    // Ein Kopf-Profil ohne Skin-Daten wurde bei Mojang nachgeschlagen (s. note_head_profile):
    // Kopf-Schluessel und die gefundene Textur-Property, None wenn es keine gibt.
    HeadProfile { key: [u8; 16], textures: Option<String> },
    // Das Nachschlagen ist gescheitert (Netz, Mojang-Limit): Kopf-Schluessel und Meldung.
    HeadProfileFailed { key: [u8; 16], meldung: String },
    // Ein Umhang ist geladen: UUID und dekodierte 64x32-RGBA-Pixel.
    CapeLoaded { uuid: [u8; 16], url: String, rgba: Vec<u8> },
    // Server-Resourcepack geladen (Pfad im Cache) oder gescheitert (Status + Meldung).
    ServerPackDownloaded { id: [u8; 16], result: Result<std::path::PathBuf, (protocol::packets::ResourcePackStatus, String)> },
    // Eine Textur konnte nicht geladen werden. `permanent` unterscheidet die beiden Faelle,
    // die sich voellig verschieden verhalten: ein nicht dekodierbares PNG bleibt bei jedem
    // Versuch nicht dekodierbar (endgueltig), ein abgerissener Download nicht (erneut
    // versuchen, s. App::skin_retries).
    SkinFailed { url: String, permanent: bool, meldung: String },
    // Profilschluessel fuer signierten Chat ist da (s. request_profile_key). Kommt ueber
    // denselben Kanal wie die Anmeldung, weil er aus demselben Konto stammt und ebenfalls
    // in einem Hintergrund-Task geholt wird.
    ProfileKey(auth::ProfileKey),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnStatus {
    Disconnected,
    Connecting,
    Connected,
}

impl ConnStatus {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "getrennt",
            Self::Connecting => "verbinde ...",
            Self::Connected => "verbunden",
        }
    }

    pub(crate) const fn color(self) -> [f32; 4] {
        match self {
            Self::Disconnected => [0.7, 0.7, 0.7, 1.0],
            Self::Connecting => [1.0, 0.85, 0.3, 1.0],
            Self::Connected => [0.4, 1.0, 0.5, 1.0],
        }
    }
}

// Was unter dem Fadenkreuz liegt (s. App::crosshair_target).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CrosshairTarget {
    Entity(i32),
    Block((i32, i32, i32), i32),
    Miss,
}

// GameMode-Index (Login/Respawn/Game Event).
pub(crate) const GAME_MODE_SURVIVAL: u8 = 0;
pub(crate) const GAME_MODE_CREATIVE: u8 = 1;
pub(crate) const GAME_MODE_ADVENTURE: u8 = 2;
pub(crate) const GAME_MODE_SPECTATOR: u8 = 3;

// Registry-IDs der Effekte in der Abbauformel (s. packets::MOB_EFFECTS).
pub(crate) const EFFECT_HASTE: i32 = 2;
pub(crate) const EFFECT_MINING_FATIGUE: i32 = 3;
pub(crate) const EFFECT_CONDUIT_POWER: i32 = 28;

// Eingabe-Aktionen, die ein Paket aus Grims Post-Liste erzeugen und deshalb NICHT im Moment
// des Tastendrucks rausgehen duerfen, sondern erst im naechsten Tick vor dem Bewegungspaket
// (s. App::flush_input_actions). Der Hotbar-Slot fehlt hier bewusst: er ist keine Aktion,
// sondern ein Zustand, der per sync_selected_slot abgeglichen wird -- sonst wuerde ein
// schneller Scroll ueber fuenf Slots fuenf Pakete erzeugen statt einem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingInputAction {
    Attack,
    // true = ganzer Stapel (Strg+Q, Vanillas Screen.hasControlDown() beim Druck).
    DropItem(bool),
    SwapOffhand,
}

// Warteschlange fuer Hotbar-Tasten -- dasselbe Prinzip wie die Rechtsklick-Schlange
// (App::use_queue). Sonst: zwei Slottasten schnell hintereinander, und der erste Slot
// wird "verschluckt" -- der Wechsel kommt nie beim Server an (nur der Endwert des Ticks wird
// gesendet, s. sync_selected_slot), und ein Klick dazwischen lief schon mit dem ZWEITEN Item.
//
// Jede Slottaste bekommt deshalb mindestens einen eigenen Tick. Klicks und Aktionen, die nach
// einer noch wartenden Slottaste kommen, haengen an genau diesem Eintrag und feuern erst,
// wenn sein Slot in der Hand ist -- die Reihenfolge Slot, Klick, Slot, Klick bleibt erhalten.
#[derive(Default)]
pub(crate) struct SlotQueue {
    pub(crate) entries: std::collections::VecDeque<QueuedSlot>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct QueuedSlot {
    pub(crate) slot: usize,
    // Rechtsklicks, die NACH dieser Slottaste kamen (landen dann in App::use_queue).
    pub(crate) uses: u32,
    // Angriffe/Wegwerfen/Tauschen nach dieser Slottaste.
    pub(crate) actions: Vec<PendingInputAction>,
}

impl SlotQueue {
    // Wie USE_QUEUE_MAX: ein paar schnelle Tasten auffangen, kein Makro puffern.
    pub(crate) const MAX: usize = 4;

    // Eine Slottaste. `sofort`: der aktuelle Slot hat seinen Tick schon gehabt und nichts
    // wartet mehr auf ihn -- dann gilt die Taste gleich (Some), sonst reiht sie sich ein.
    pub(crate) fn press(&mut self, slot: usize, sofort: bool) -> Option<usize> {
        if sofort && self.entries.is_empty() {
            return Some(slot);
        }
        if self.entries.len() < Self::MAX {
            self.entries.push_back(QueuedSlot { slot, uses: 0, actions: Vec::new() });
        }
        None
    }

    // Ein Mausrad-Schritt, waehrend Slottasten warten muessen: vom zuletzt vorgemerkten Slot aus
    // weiter. Haengt an dem Eintrag nichts, wird er nur umgestellt -- Durchscrollen ueber
    // mehrere Slots ergibt einen Wechsel, wie ohne Schlange.
    pub(crate) fn scroll(&mut self, held: usize, up: bool) {
        let basis = self.entries.back().map_or(held, |e| e.slot) as i32;
        let slot = (basis + if up { -1 } else { 1 }).rem_euclid(9) as usize;
        match self.entries.back_mut() {
            Some(letzter) if letzter.uses == 0 && letzter.actions.is_empty() => letzter.slot = slot,
            _ => {
                self.press(slot, false);
            }
        }
    }

    // Ein Rechtsklick: true, wenn er an einer wartenden Slottaste haengt.
    pub(crate) fn add_use(&mut self, max: u32) -> bool {
        let Some(letzter) = self.entries.back_mut() else { return false };
        letzter.uses = (letzter.uses + 1).min(max);
        true
    }

    pub(crate) fn add_action(&mut self, action: PendingInputAction) -> bool {
        let Some(letzter) = self.entries.back_mut() else { return false };
        if letzter.actions.len() < 64 {
            letzter.actions.push(action);
        }
        true
    }

    // Zum Tickbeginn: der naechste Slot, wenn der aktuelle fertig ist (`frei`).
    pub(crate) fn next_for_tick(&mut self, frei: bool) -> Option<QueuedSlot> {
        if frei {
            self.entries.pop_front()
        } else {
            None
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

// Die Zahlenregler des Sprachchat-Moduls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum VoiceKnob {
    Volume,
    MicGain,
    Threshold,
    // Nachlauf der gerade gewaehlten Aktivierungsart.
    Delay,
    OutputBuffer,
    PacketThreshold,
}

// Schalter und Auswahlen des Sprachchat-Moduls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum VoiceFlag {
    Activation,
    Agc,
    Denoise,
    AudioType,
    MicDevice,
    Muted,
    Disabled,
    MuteOnJoin,
    NametagIcons,
    HudIcons,
    Monitor,
}

// Was unter dem Zeiger klingt (s. update_hover_sound): ein Knopf, oder im Skin-Tab ein Ziel
// ohne Knopf -- Kachel (auf dem Bild mit dem Zahnrad eigenes Ziel), Zeile des Dateifensters
// (bei Ordnern Kaestchen und Rest getrennt), "Alles an", "Fertig". Sonst eine mit Hover
// gezeichnete Flaeche (s. App::hover_level).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoverTarget {
    Drawn((i32, i32, i32, i32)),
    Button(ButtonAction),
    SkinTile(skin_tab::Column, usize, bool),
    SkinFileRow(usize, bool),
    SkinFilesAll,
    SkinFilesDone,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonAction {
    // Tut nichts: der schon gewaehlte Knopf einer Auswahl (s. ModuleRow::Pick).
    Nothing,
    // Navigation zwischen den Menue-Screens.
    OpenScreen(MenuScreen),
    // Leiste oben: Zahnrad (Einstellungen auf/zu) und Glocke (Benachrichtigungen auf/zu).
    ToggleSettings,
    // Farbfeld angeklickt: Farbfenster fuer diese Farbe auf (s. ColorPopup).
    OpenColorPicker(ColorTarget),
    // Aufklappliste einer Auswahl ohne eigene Durchschalt-Aktion (s. App::choice_of).
    OpenChoice(Choice),
    // Addon-Module (Index in ADDON_MODULES) und ihre Einstellungen (Index in der Anmeldung):
    // Hauptschalter, Schalter, Regler (hoch?), Knopf.
    ToggleAddonModule(u16),
    AddonToggle(u16, u16),
    AddonStep(u16, u16, bool),
    AddonButton(u16, u16),
    // Theme: Preset uebernehmen (Index in theme::PRESETS), eigenes waehlen (Index in
    // custom_themes) oder ein neues anlegen ("+", oeffnet das Namensfenster).
    ApplyThemePreset(usize),
    SelectCustomTheme(usize),
    NewCustomTheme,
    // Reiter oben auf einer Einstellungsseite: zum Abschnitt springen (s. SettingsPage).
    SettingsSection(u8),
    // Uhr in der Leiste oben angeklickt: "laeuft seit" darunter an/aus.
    ToggleUptime,
    // Server-Tabs in der Leiste oben (s. app/tabs.rs): nach vorn holen, schliessen, neu.
    SelectTab(usize),
    CloseTab(usize),
    NewTab,
    // Logo in der Kategorienleiste: Index in SIDEBAR_LINKS, oeffnet die Seite im Browser.
    OpenLink(usize),
    // Leiste unten der Server-Seite: Quick-Connect-Fenster oeffnen.
    OpenQuickConnect,
    // Liste der Aufnahmen auf- und zuklappen, Aufnahme `i` abspielen (s. app/replay.rs).
    ToggleRecordings,
    PlayRecording(usize),
    // Konto in der Leiste oben: Kontenliste auf/zu (offen wird der Knopf zum Acc-Manager).
    ToggleAccountMenu,
    ToggleNotificationPanel,
    Back,
    Connect,
    Disconnect,
    // Online-Mode-Login (Connect-Screen): Microsoft-Device-Code starten, ein Eingabefeld
    // (Token bzw. Refresh-Token) auf-/zuklappen, das Eingegebene uebernehmen, abmelden.
    MicrosoftLogin,
    OpenInputField(InputKind),
    SubmitInputField,
    Logout,
    // Server-Liste (Connect-Screen, rechts): Karte anklicken waehlt sie aus, ein zweiter Klick
    // verbindet; "Neuer Eintrag" (Leiste unten) oeffnet das Fenster fuer einen neuen Eintrag.
    ConnectToServer(usize),
    NewServer,
    // Gespeichertes Konto (Index in App::accounts.list) anklicken: damit anmelden.
    UseAccount(usize),
    ToggleTearing,
    ToggleHud,
    FrameLatencyDown,
    FrameLatencyUp,
    FpsLimitPrev,
    FpsLimitNext,
    SensitivityDown,
    SensitivityUp,
    SensitivityYDown,
    SensitivityYUp,
    ToggleSensSplit,
    // Sens-Rechner oeffnen bzw. dessen Ergebnis als Kamera-Empfindlichkeit uebernehmen.
    OpenSensCalc,
    // Knopf neben dem Renderpfad, der gerade ausgeblendet ist (s. buttons()).
    #[allow(dead_code)]
    OpenShaderSettings,
    // Configs-Seite: zur Config Nummer i (alphabetisch, s. assets::list_configs) wechseln,
    // das Namensfeld fuer Neu/Duplizieren/Umbenennen oeffnen, die aktive Config loeschen
    // (erster Klick fragt nach, s. App::config_delete_armed).
    SelectConfig(usize),
    EditConfig(ConfigEdit),
    OpenConfigFolder,
    // Pausemenue: die Links des Servers als Dialog.
    ServerLinks,
    Reconnect,
    DeleteConfig,
    ScatteringPrev,
    ScatteringNext,
    SunSizePrev,
    SunSizeNext,
    ToggleBlockShadows,
    LightmapDebugPrev,
    LightmapDebugNext,
    BlockSoftnessPrev,
    BlockSoftnessNext,
    // Lichtstaerken: Seite oeffnen; Faktor der Quelle Nummer i in light_source_blocks().
    OpenLightLevels,
    // Partikel-Anpassung oeffnen: true = die eigenen Partikel, false = die anderen.
    OpenParticleSettings(bool),
    // Unterseite Animationen (MenuScreen::SettingsAnimations) und ihr Hauptschalter.
    OpenAnimSettings,
    ToggleAnims,
    // Partikel-Anpassung (eigene?, Index = particles::KINDS).
    ParticleToggle(bool, usize),
    ParticleAllAtOnce(bool, usize),
    // (eigene?, Art, Spalte 0 Dauer / 1 Deckkraft / 2 Tempo, hoeher?)
    ParticleStep(bool, usize, u8, bool),
    LightFactorPrev(usize),
    LightFactorNext(usize),
    ApplySensCalc,
    CursorSensitivityDown,
    CursorSensitivityUp,
    CursorSizeDown,
    CursorWidthDown,
    CursorWidthUp,
    HoldConfirmDown,
    HoldConfirmUp,
    ReconnectHoldDown,
    ReconnectHoldUp,
    CursorSizeUp,
    ToggleCursorCircle,
    FovDown(Perspective),
    // Knopf "Ueberspringen" neben dem Sichtfeld einer 3.-Person-Ansicht.
    SkipPerspective(Perspective),
    FovUp(Perspective),
    CameraDistanceDown(Perspective),
    CameraDistanceUp(Perspective),
    CameraYOffsetDown(Perspective),
    CameraYOffsetUp(Perspective),
    CycleProjection,
    CycleTearPosition,
    CycleRenderPath,
    TearTimingDown,
    TearTimingUp,
    TearReserveDown,
    TearReserveUp,
    // Partikel-Stufe der eigenen (true) bzw. der anderen Partikel.
    CycleParticles(bool),
    ToggleStableNumbers,
    ToggleCapitals,
    // Vanilla-Schatten von Kaestchen i: 1 oder 1/2 Pixel versetzt.
    ToggleShadowHalf(usize),
    CycleWallpaper,
    PickWallpaper,
    // Sprach-Tab: Index in LANGUAGE_ROWS (0 = alle Bereiche auf einmal).
    CycleLanguage(usize),
    ToggleLatinMacrons,
    ToggleLoanwords,
    ChatWidthPrev,
    ChatWidthNext,
    ChatLinesPrev,
    ChatLinesNext,
    ChatLinesOpenPrev,
    ChatLinesOpenNext,
    ChatBackgroundPrev,
    ChatBackgroundNext,
    ChatOpenDimPrev,
    ChatOpenDimNext,
    ChatFadePrev,
    ChatFadeNext,
    ChatLineSpacingPrev,
    ChatLineSpacingNext,
    // Chat im Spiel: false = Java, true = Bedrock.
    ChatStyle(bool),
    // Nie mehr konstruiert, seit der CMAA2-Umschalter in buttons() auskommentiert ist (s.
    // CMAA2_ROW-Kommentar) -- Variante bleibt fuer eine spaetere Reaktivierung stehen.
    #[allow(dead_code)]
    ToggleCmaa2,
    MeshBudgetDown,
    MeshBudgetUp,
    MaxRenderDistanceDown,
    MaxRenderDistanceUp,
    ChunkUpdateThreadsDown,
    ChunkUpdateThreadsUp,
    CrosshairColorPrev,
    CrosshairColorNext,
    CrosshairBoostPrev,
    CrosshairBoostNext,
    ToggleCrosshairOutline,
    ToggleCrosshairRingOutline,
    // Form des Fadenkreuzes (s. CROSSHAIR_SHAPES, zuletzt "Eigene") und ihre Groesse.
    CrosshairSizePrev,
    CrosshairSizeNext,
    // Pixel-Editor fuers eigene Fadenkreuz (s. crosshair_editor).
    OpenCrosshairPainter,
    CrosshairPainterDone,
    CrosshairPainterClear,
    CrosshairPainterStandard,
    CrosshairPainterSymmetry,
    ToggleEffectsHud,
    ToggleCaveCulling,
    ToggleMipmapping,
    ToggleEffectsInventory,
    // Detaillierte Effektanzeige: Stil (true = detailliert), Ausrichtung (EFFECTS_ALIGNS),
    // Icon, Name in Effektfarbe, Timer-Art (EFFECTS_TIMER_STYLES), Timer in Effektfarbe.
    EffectsDetailed(bool),
    EffectsAlign(u8),
    ToggleEffectsIcon,
    ToggleEffectsHideAmbient,
    ToggleEffectsName,
    EffectsTimerBlink(u8),
    EffectsNameColor(bool),
    EffectsTimerStyle(u8),
    EffectsTimerColor(bool),
    // GUI-Skalierung: 0 = Auto, sonst feste Stufen (s. App::gui_scale_of).
    InventoryScalePrev,
    InventoryScaleNext,
    HotbarScalePrev,
    HotbarScaleNext,
    HotbarDurabilityPrev,
    HotbarDurabilityNext,
    ToggleHotbarDurabilityMode,
    ToggleHotbarAirBar,
    ChatScalePrev,
    ChatScaleNext,
    UiPitchDown,
    UiPitchUp,
    InventoryTransparencyPrev,
    ToggleInventoryCursorToSlot,
    ToggleCreativeOpTab,
    // Sonstiges > Server-Bildschirme: Unterseite oeffnen, Schliessen-Schalter einer Zeile,
    // Oeffnen-Modus von Eintrag i (s. SERVER_OPEN_*).
    OpenServerScreens,
    ToggleServerClose(usize),
    ServerOpen(usize, u8),
    ToggleNoChatClear,
    ToggleSignEditInWorld,
    NoSlotSet(u8),
    NoRotateSet(u8),
    ToggleInventoryMergeHover,
    TogglePotionMarkStrong,
    TogglePotionMarkLong,
    TogglePotionMarkWeak,
    InventoryTransparencyNext,
    GlintSpeedPrev,
    GlintSpeedNext,
    GlintStrengthPrev,
    GlintStrengthNext,
    PlayerListScalePrev,
    PlayerListScaleNext,
    MenuBackdropPrev,
    MenuBackdropNext,
    ScrollInertiaPrev,
    ScrollInertiaNext,
    ScrollDistancePrev,
    ScrollDistanceNext,
    EntityDistancePrev,
    EntityDistanceNext,
    BlockEntityDistancePrev,
    BlockEntityDistanceNext,
    AudioTestTone,
    AudioSpatialTest,
    // Eigene Untergrenze von pipewire-pulse in Frames setzen, None = entfernen.
    PipewireLimit(Option<u32>),
    ToggleAudioAutoOptimize,
    MasterVolumeDown,
    MasterVolumeUp,
    // Index in VOLUME_CATEGORIES.
    CategoryVolumeDown(usize),
    CategoryVolumeUp(usize),
    // Index in ui_sound::UiSound::ALL.
    UiSoundVolumeDown(usize),
    UiSoundVolumeUp(usize),
    // HUD-Editor: Ausrichtung, Einstellungsfenster schliessen, Groesse, das offene Element auf
    // seine Standardposition/-werte zuruecksetzen.
    // Ausrichtung von Titel bzw. Actionbar: 0 links, 1 mittig, 2 rechts.
    HudAlign(HudElement, u8),
    HudDeselect,
    // Groesse eines HUD-Elements (Titel, Untertitel, Spielerliste) um einen Schritt.
    HudScaleStep(HudElement, bool),
    ResetHudElement,
    // HUD-Editor oeffnen, gleich mit dem Fenster dieses Elements (Link aus einem Modul).
    OpenHudConfig(HudElement),
    ToggleTearFree,
    ToggleBlockRngRotation,
    ToggleJumpCooldown,
    ToggleSprintOnStart,
    ToggleCapsLockBlocked,
    ToggleSnapTap,
    ToggleUseWhileMining,
    ToggleRocketRepeat,
    ToggleExtinguishDcGhost,
    CycleHoldUseRepeat,
    ToggleSneakEdgeProtection,
    // Aktion + Slot. Ein Slot jenseits der belegten zaehlt als "neue Belegung anlegen" (der
    // "+"-Knopf), sonst wird genau dieser Slot neu belegt.
    Rebind(KeyBindAction, usize),
    // Fenster mit allen Belegungen einer Aktion (bis zu MAX_BINDS_PER_ACTION) auf bzw. zu.
    OpenKeybindAdvanced(KeyBindAction),
    CloseKeybindAdvanced,
    ToggleKeybindMode(KeyBindAction),
    Respawn,
    // Den Skin-Ordner im Dateimanager oeffnen (s. App::open_skins_dir).
    OpenSkinFolder,
    // Den gewaehlten Skin-Stapel sofort von der Platte neu laden (s. App::reload_resource_pack).
    ReloadSkins,
    // Zips im Skin-Ordner von selbst entpacken an/aus (s. Settings::skin_auto_unzip).
    ToggleSkinAutoUnzip,
    // Modul-Menue (s. modules.rs): Module an-/abschalten. Kein OpenScreen -- das Menue ist
    // ein In-Welt-Overlay, kein Menue-Screen.
    ToggleModuleTotem,
    ToggleModuleTotemSound,
    // Alles aus Toteming Pro Max (s. toteming.rs) -- eine Variante mit eigener Aktion statt
    // zwanzig einzelner hier.
    Toteming(toteming::Action),
    // Linke Liste: welches Modul die rechte Spalte gerade zeigt (s. App::module_menu_selected).
    SelectModule(ModuleId),
    // Kategorie links im Modul-Menue waehlen (zeigt deren Kacheln).
    SelectModuleCategory(ModuleCategory),
    // Aus den Einstellungen eines Moduls zurueck zu den Kacheln.
    ModuleBack,
    ToggleModuleHitbox,
    ToggleModuleHitboxSmoothed,
    ToggleModuleHitboxRaw,
    ToggleModuleHitboxDotted,
    ToggleModuleHitboxItems,
    ToggleModuleHitboxSelf,
    HitboxDensityPrev,
    HitboxDensityNext,
    HitboxOpacityPrev,
    HitboxOpacityNext,
    // Blockumriss-Modul (s. ModuleId::BlockOutline).
    ToggleModuleBlockOutline,
    BlockOutlineEditor,
    BlockOutlineWidthPrev,
    BlockOutlineWidthNext,
    ToggleBlockOutlineHideMining,
    // Blockumriss: Komplementaer (true) oder Einfarbig.
    SetBlockOutlineComplementary(bool),
    // Schadens-Faerbung (Video-Tab, s. Settings::damage_tint_*); die Farbe selbst im Farbfenster.
    ToggleDamageTintSkinOnly,
    ToggleModuleShields,
    ToggleModuleArrows,
    ToggleModuleArmorHud,
    ToggleModuleTeams,
    // Teams-Modul je Seite: Ueberschreiben, Farbe vom Namensschild (true) oder fest,
    // Deckkraft und Dichte (true = mehr), gepunktet.
    TeamToggle(teams::Side),
    // Eine Erkennung an/aus (Index in teams::Matchers::LABELS).
    TeamMatch(usize),
    // Eigene Teamfarbe festlegen (None = automatisch).
    TeamOwnColor(Option<u32>),
    TeamNametag(teams::Side, bool),
    TeamOpacity(teams::Side, bool),
    TeamDensity(teams::Side, bool),
    TeamDotted(teams::Side),
    ToggleModuleKeystrokes,
    // Video-Tab: Hurt-Cam und Feuer.
    ToggleHurtCam,
    ToggleArmorTrims,
    ToggleViewModel,
    HurtCamStrengthPrev,
    HurtCamStrengthNext,
    FireMode(u8),
    FireHeightPrev,
    FireHeightNext,
    // Deckkraft der nicht gedrueckten Keystrokes-Tasten.
    KeysOpacityPrev,
    KeysOpacityNext,
    // Session-Info: Deckkraft der Tafel, eine Zeile an/aus (session_info::LINE_*), Zaehler auf 0.
    SessionOpacity(bool),
    ToggleSessionLine(u8),
    ResetSession,
    ToggleModuleSessionInfo,
    // Video > Tod: Todesanimation weiterschalten (DEATH_ANIM_NAMES).
    CycleDeathAnimation,
    // Ruestungs-HUD: Ausrichtung (armor_hud::LAYOUT_*) und Ruckrichtung (0 = oben/links,
    // 1 = unten/rechts) fuer die gerade gewaehlte Ausrichtung.
    ArmorHudLayout(u8),
    ArmorHudSort(u8),
    ArmorHudGapPrev,
    ArmorHudGapNext,
    ArmorHudNumberScalePrev,
    ArmorHudNumberScaleNext,
    ArmorHudNumberXPrev,
    ArmorHudNumberXNext,
    ArmorHudNumberYPrev,
    ArmorHudNumberYNext,
    ToggleArrowIndicator,
    ToggleArrowNumberOnly,
    ToggleArmorHudGreen,
    ToggleArmorHudRing,
    // Treffer der Einstellungssuche anklicken (Index in App::search_hits).
    SearchHit(u16),
    ToggleShoulderCam,
    ShoulderOffsetPrev,
    ShoulderOffsetNext,
    ToggleShoulderCrosshair,
    ToggleShoulderFade,
    ToggleShoulderDecoupled,
    ToggleModuleFreelook,
    FreelookView(u8),
    ToggleArrowWhite,
    ArrowOpacityPrev,
    ArrowOpacityNext,
    ToggleArrowTrail,
    TrailHoldPrev,
    TrailHoldNext,
    TrailFadePrev,
    TrailFadeNext,
    ToggleShieldCdSelf,
    ToggleShieldCdPlayers,
    ToggleShieldHoldPlayers,
    ShieldColorEditor(ShieldColor),
    CycleShieldMark(ShieldColor),
    CycleShieldSelfMode,
    CycleShieldCdTextStyle,
    CycleArrowTextStyle,
    CycleArrowEmptyFeedback,
    ToggleModuleShowMarkers,
    ToggleModuleSoundDebug,
    ToggleModuleHandDebug,
    ToggleModuleConnectionDebug,
    ToggleModuleDelay,
    LagDelayPrev,
    LagDelayNext,
    ToggleModuleDiscordStatus,
    ToggleModuleInputLog,
    ToggleModuleCoralRecord,
    ToggleModuleAutoclicker,
    ToggleModuleSaturationHud,
    SaturationHeightPrev,
    SaturationHeightNext,
    // (welcher Regler: false = Grund, true = Raster; hoch?)
    SaturationAlpha(bool, bool),
    ToggleModuleEgoName,
    // Oeffnet das Namensfenster fuer den Alias.
    EditEgoAlias,
    CycleEgoDeco,
    CycleEgoShadow,
    // Helligkeit des Regenbogens: (Schatten?, hoch?).
    EgoLight(bool, bool),
    // hoch?
    EgoSpeed(bool),
    CycleEgoSound,
    EgoVolume(bool),
    AutoclickerCpsPrev,
    AutoclickerCpsNext,
    ToggleModuleColor,
    ToggleModuleSustainability,
    ToggleSustainBackground,
    // Ein Schalter im Abschnitt Animationen des Video-Tabs (Bit aus world::more_anims).
    ToggleAnimFlag(u32),
    ToggleSustainPause,
    ToggleModuleChunkSaver,
    ToggleChunkSaverRenderSaved,
    ChunkSaverThreadsPrev,
    ChunkSaverThreadsNext,
    ChunkSaverThresholdPrev,
    ChunkSaverThresholdNext,
    ChunkSaverDistancePrev,
    ChunkSaverDistanceNext,
    ColorSaturationPrev,
    ColorSaturationNext,
    // Discord-Modul: Application-ID aus der Zwischenablage uebernehmen.
    DiscordPasteClientId,
    ShieldColorPrev(ShieldColor, DamageTintAxis),
    ShieldColorNext(ShieldColor, DamageTintAxis),
    DamageTintTicksPrev,
    DamageTintTicksNext,
    // Crit-Farbe im Video-Tab (s. CRIT_COLOR_ROW). Dieselben vier Achsen wie die gewoehnliche
    // Schadens-Faerbung -- es ist dieselbe Art Farbe, nur fuer den anderen Anlass.
    ToggleModuleCritColor,
    ToggleModuleSkyColor,
    ToggleModuleGhostTolerance,
    ToggleModuleCrystalOptimizer,
    ToggleModuleConsumableOptimizer,
    EatHoldThroughPrev,
    EatHoldThroughNext,
    ToggleEatSlotLock,
    ToggleEatAggressivePrediction,
    ToggleModuleUseTimer,
    ToggleModuleNoToolBreak,
    ToggleModuleBotcheckPass,
    ToggleModuleDynamicFire,
    ToggleModuleZoom,
    ToggleModuleVoiceChat,
    // Sprachchat-Regler: (welcher, hoch?) bzw. Schalter/Auswahl.
    VoiceStep(VoiceKnob, bool),
    VoiceFlag(VoiceFlag),
    ZoomFactorPrev,
    ZoomFactorNext,
    ZoomSensPrev,
    ZoomSensNext,
    NoToolBreakReservePrev,
    NoToolBreakReserveNext,
    SmoothLightingDown,
    SmoothLightingUp,
    MotionBlurDown,
    MotionBlurUp,
    // Inventar > Animation (Settings::slot_hover_fade_ms, swap_anim_ms).
    SlotHoverFadeDown,
    SlotHoverFadeUp,
    SwapAnimDown,
    SwapAnimUp,
    GhostTolerancePrev,
    GhostToleranceNext,
    CycleSkyMode,
    ToggleSkySun,
    ToggleSkyMoon,
    ToggleSkyStars,
    ToggleSkyFixedTime,
    SkyTimePrev,
    SkyTimeNext,
    ToggleModuleWeather,
    CycleWeatherMode,
    WeatherRadiusPrev,
    WeatherRadiusNext,
    WeatherRadiusInnerPrev,
    WeatherRadiusInnerNext,
    // Farbfeld angeklickt: klappt den HSV-Editor dieser Variante auf bzw. wieder zu.
    HitboxColorEditor(HitboxVariant),
    // Eigene Art der Hitbox-Anzeige an/aus (s. hitbox_kinds).
    ToggleHitboxKind(hitbox_kinds::HitboxKind),
    // Eigene Farbe und Deckkraft dieser Art statt der von "Geglättet".
    ToggleHitboxKindOverride(hitbox_kinds::HitboxKind),
    CloseModuleMenu,
}
