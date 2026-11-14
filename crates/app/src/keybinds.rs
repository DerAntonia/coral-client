// Tastenbelegung: Aktionen, Modi, Tasten- und Maus-Namen.

use crate::*;

// Nur Aktionen, die dieser Client tatsaechlich umsetzt (Bewegung) -- Vanilla kennt viel
// mehr Tastenbelegungen (Inventar, Angriff, Item benutzen, Hotbar 1..9, Chat, ...), aber
// fuer eine Taste ohne zugehoerige Funktion gaebe es hier nichts umzubelegen: ein Rebind-
// Eintrag, der nichts tut, waere nur eine Attrappe. Kommt automatisch dazu, sobald die
// jeweilige Funktion existiert.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyBindAction {
    Forward,
    Backward,
    Left,
    Right,
    Jump,
    Sneak,
    Sprint,
    // Kameraperspektive umschalten (Vanillas F5). Anders als alle anderen hier KEINE
    // Halte-/Umschalt-Aktion, sondern ein einmaliger Tastendruck -- s. is_press_action.
    TogglePerspective,
    // Inventar oeffnen/schliessen (Vanillas E) -- ebenfalls eine Einmal-Aktion.
    OpenInventory,
    // Angriff/Abbauen. War bisher fest auf der linken Maustaste verdrahtet; jetzt normal
    // belegbar (Standard bleibt die linke Maustaste).
    Attack,
    // Gehaltenes Item wegwerfen (Vanillas Q) bzw. mit der Zweithand tauschen (F).
    DropItem,
    SwapOffhand,
    // Chat oeffnen (T) bzw. mit vorangestelltem Schraegstrich fuer einen Befehl.
    OpenChat,
    OpenCommand,
    // F3-Debug-Overlay.
    ToggleDebugOverlay,
    // Hotbar-Platz 1..9 direkt waehlen. Neun eigene Aktionen statt einer Sonderbehandlung
    // der Zifferntasten -- so laesst sich das genauso umbelegen wie alles andere.
    Hotbar(u8),
    // Einen Hotbar-Platz nach links bzw. rechts (Vanilla: fest am Mausrad). Einmal-Aktionen;
    // Standard ist das Mausrad (s. BindMouse::WheelUp/WheelDown).
    HotbarPrev,
    HotbarNext,
    // Hauptlautstaerke in 1-%-Schritten, ueberall (auch im Menue) und mit Wiederholung beim
    // Halten (s. App::tick_volume_keys). Standardmaessig unbelegt.
    VolumeUp,
    VolumeDown,
    // Modul-Menue oeffnen/schliessen (s. modules.rs). Einmal-Aktion wie das Inventar.
    OpenModuleMenu,
    // --- Nur bei offenem Inventar/Container (s. is_inventory_action) ------------------
    // Stapel verschieben: Vanillas Shift-Klick (SlotActionType.QUICK_MOVE). Bei uns eine
    // eigene, frei belegbare Aktion statt fest an der Schleichtaste zu haengen.
    InventoryMoveStack,
    // Den Stapel unter dem Zeiger wegwerfen (Vanillas Q im Inventar, SlotActionType.THROW).
    // Getrennt von DropItem, das im Spiel das Item aus der Hand wirft.
    InventoryDropItem,
    // Rechtsklick: Item benutzen bzw. Block platzieren. Fuehrt BEIDES -- einen Haltezustand
    // (Vanillas Dauerfeuer alle 4 Ticks) UND jeden einzelnen Druck (s. App::use_queue).
    Use,
    // Tab-Liste (Vanillas keyPlayerList): sichtbar, solange die Taste unten ist -- oder per
    // Umschalten dauerhaft.
    PlayerList,
    // Vanillas F1: blendet das HUD aus (Hotbar, Leisten, Chat, Fadenkreuz, Hand ...).
    HideHud,
    // Frame-Profiler an/aus (s. profiler.rs) -- misst, wo die Frame-Zeit hingeht.
    ToggleProfiler,
    // Zoom-Modul (s. ModuleId::Zoom): solange aktiv, wird das Sichtfeld verengt. Haelt einen
    // Zustand, laesst sich also auch auf Umschalten stellen.
    Zoom,
    // Freelook-Modul (s. ModuleId::Freelook): solange aktiv, dreht die Maus nur die Kamera.
    // Halten oder Umschalten wie der Zoom.
    Freelook,
    // Schulterkamera: auf die andere Schulter wechseln (Einmal-Aktion).
    SwapShoulder,
    // Sprachchat (Modul): solange gehalten, geht das Mikrofon raus.
    PushToTalk,
    // Inventar: solange gehalten, zeigt jedes Item unter dem Zeiger den ausfuehrlichen
    // Tooltip (Haltbarkeit, ID, ...), auch eines, das sonst keinen braucht.
    InspectItem,
    // Sprachchat: Mikrofon stumm / Sprachchat taub umschalten, Fluestern (gehalten).
    VoiceMute,
    VoiceDeafen,
    VoiceWhisper,
    // Sprachchat: das Modul-Menue direkt auf den Sprachchat-Einstellungen oeffnen.
    VoiceConfig,
    // Mittelklick: den Block (bzw. das Entity) unter dem Fadenkreuz in die Hand nehmen.
    PickBlock,
    // Das gehaltene Item in der Ego-Hand ansehen (wie Inspect in Counter-Strike, s.
    // world::item_inspect). Einmal-Aktion.
    InspectHeld,
    // Debug-Modul Hand-Debug an/aus (s. ModuleId::HandDebug).
    ToggleHandDebug,
    // coral-clip: die letzten Sekunden als Aufnahme speichern (s. app/clip.rs). Einmal-Aktion.
    SaveClip,
    // Zuschauer: Fluggeschwindigkeit eine Stufe runter/hoch (s. SPECTATOR_SPEED_STEPS).
    // Standardmaessig unbelegt, das Mausrad tut dasselbe.
    SpectatorSlower,
    SpectatorFaster,
    // Spielmodus-Rad (s. gamemode_wheel.rs): solange gehalten offen, beim Loslassen gilt der
    // Modus unter dem Zeiger, in der Mitte der vorige.
    GameModeWheel,
    // Wie osus Neustart-Taste: so lange gehalten, wie Settings::reconnect_hold vorgibt, verbindet
    // neu (s. App::tick_reconnect_hold); waehrenddessen dunkelt das Bild ab.
    Reconnect,
}

impl KeyBindAction {
    pub(crate) const ALL: [Self; 52] = [
        Self::Forward,
        Self::Backward,
        Self::Left,
        Self::Right,
        Self::Jump,
        Self::Sneak,
        Self::Sprint,
        Self::Attack,
        Self::DropItem,
        Self::SwapOffhand,
        Self::OpenInventory,
        Self::OpenChat,
        Self::OpenCommand,
        Self::TogglePerspective,
        Self::ToggleDebugOverlay,
        Self::HideHud,
        Self::ToggleProfiler,
        Self::Hotbar(1),
        Self::Hotbar(2),
        Self::Hotbar(3),
        Self::Hotbar(4),
        Self::Hotbar(5),
        Self::Hotbar(6),
        Self::Hotbar(7),
        Self::Hotbar(8),
        Self::Hotbar(9),
        Self::HotbarPrev,
        Self::HotbarNext,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::OpenModuleMenu,
        Self::InventoryMoveStack,
        Self::InventoryDropItem,
        Self::Use,
        Self::PlayerList,
        Self::Zoom,
        Self::Freelook,
        Self::SwapShoulder,
        Self::PushToTalk,
        Self::InspectItem,
        Self::VoiceMute,
        Self::VoiceDeafen,
        Self::VoiceWhisper,
        Self::VoiceConfig,
        Self::PickBlock,
        Self::SpectatorSlower,
        Self::SpectatorFaster,
        Self::GameModeWheel,
        Self::Reconnect,
        Self::InspectHeld,
        Self::ToggleHandDebug,
        Self::SaveClip,
    ];

    // Aktionen, die NUR bei offenem Inventar/Container gelten. Sie duerfen sich eine Taste
    // mit einer Welt-Aktion teilen -- genau das macht Vanilla: Shift ist Schleichen UND
    // Stapel-Verschieben, Q ist Wegwerfen im Spiel UND im Inventar; die beiden Kontexte
    // schliessen sich aus.
    pub(crate) const fn is_inventory_action(self) -> bool {
        matches!(self, Self::InventoryMoveStack | Self::InventoryDropItem)
    }

    // Welche Welt-Aktion diese Inventar-Aktion vertritt, solange sie KEINE eigene Taste hat.
    // Ohne eigene Belegung gilt also einfach die Taste des Gegenstuecks -- Q wirft dann auch
    // im Inventar, Shift verschiebt dort Stapel, genau wie in Vanilla.
    //
    // Damit ist die Zweitbelegung eine bewusste Ausnahme statt der Voreinstellung. Vorher war
    // beides fest doppelt belegt, und wer die Taste noch fuer etwas anderes vergab (etwa
    // Linksshift zusaetzlich auf Hotbar-Platz 1), loeste mit einem Druck zwei Dinge aus.
    pub(crate) const fn inventory_fallback(self) -> Option<Self> {
        match self {
            Self::InventoryMoveStack => Some(Self::Sneak),
            Self::InventoryDropItem => Some(Self::DropItem),
            _ => None,
        }
    }

    // Einmal-Aktionen: sie loesen beim Druck etwas aus, statt einen Zustand zu halten. Fuer
    // sie ergibt die Halten/Umschalten-Wahl keinen Sinn (beides waere dasselbe), und sie
    // haben keinen "aktiv"-Zustand, den action_active zurueckgeben koennte.
    // Hat die Aktion einen Halten/Umschalten-Modus? Alle haltenden, dazu die Perspektive-
    // Taste: Halten ist dort Quicklook (s. Settings::quicklook_target).
    pub(crate) fn has_mode(self) -> bool {
        (!self.is_press_action() && self != Self::Reconnect) || self == Self::TogglePerspective
    }

    pub(crate) const fn is_press_action(self) -> bool {
        !matches!(
            self,
            Self::Forward
                | Self::Backward
                | Self::Left
                | Self::Right
                | Self::Jump
                | Self::Sneak
                | Self::Sprint
                // Stapel-Verschieben ist ein Modifikator, kein Auslöser: er wird beim Klick
                // ABGEFRAGT, muss also einen Zustand halten (und kann darum auch auf
                // Umschalten gestellt werden).
                | Self::InventoryMoveStack
                // Rechtsklick haelt einen Zustand (Dauerfeuer beim Halten) und zaehlt
                // zusaetzlich jeden Druck -- s. apply_key_action.
                | Self::Use
                | Self::PlayerList
                | Self::Zoom
                | Self::Freelook
                | Self::PushToTalk
                | Self::InspectItem
                | Self::VoiceWhisper
                | Self::GameModeWheel
                // Haelt einen Zustand: erst nach der Bestaetigungsdauer wird neu verbunden.
                | Self::Reconnect
        )
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Forward => "Vorwärts",
            Self::Backward => "Rückwärts",
            Self::Left => "Links",
            Self::Right => "Rechts",
            Self::Jump => "Springen",
            Self::Sneak => "Schleichen",
            Self::Sprint => "Sprinten",
            Self::TogglePerspective => "Perspektive (F5)",
            Self::OpenInventory => "Inventar",
            Self::Attack => "Angreifen/Abbauen",
            Self::Use => "Benutzen/Platzieren",
            Self::PlayerList => "Spielerliste (Tab)",
            Self::DropItem => "Item wegwerfen",
            Self::SwapOffhand => "Mit Zweithand tauschen",
            Self::OpenChat => "Chat",
            Self::OpenCommand => "Befehl",
            Self::ToggleDebugOverlay => "Debug-Overlay (F3)",
            Self::HideHud => "HUD verstecken (F1)",
            Self::ToggleProfiler => "Frame-Profiler",
            Self::Zoom => "Zoom (Modul)",
            Self::Freelook => "Freelook (Modul)",
            Self::SwapShoulder => "Schulter wechseln",
            Self::PushToTalk => "Sprachchat: Sprechen",
            Self::InspectItem => "Inventar: Item untersuchen",
            Self::InspectHeld => "Item in der Hand ansehen",
            Self::ToggleHandDebug => "Hand-Debug an/aus",
            Self::SaveClip => "Clip speichern (Modul)",
            Self::PickBlock => "Block auswählen",
            Self::SpectatorSlower => "Zuschauer: langsamer",
            Self::SpectatorFaster => "Zuschauer: schneller",
            Self::GameModeWheel => "Spielmodus-Rad",
            Self::Reconnect => "Reconnect (halten)",
            Self::VoiceMute => "Sprachchat: Mikrofon stumm",
            Self::VoiceDeafen => "Sprachchat: taub",
            Self::VoiceWhisper => "Sprachchat: Flüstern",
            Self::VoiceConfig => "Sprachchat: Einstellungen öffnen",
            Self::OpenModuleMenu => "Modul-Menü",
            Self::InventoryMoveStack => "Inventar: Stapel verschieben",
            Self::InventoryDropItem => "Inventar: wegwerfen",
            Self::HotbarPrev => "Hotbar nach links",
            Self::HotbarNext => "Hotbar nach rechts",
            Self::VolumeUp => "Lautstärke hoch (1 %)",
            Self::VolumeDown => "Lautstärke runter (1 %)",
            Self::Hotbar(slot) => match slot {
                1 => "Hotbar 1",
                2 => "Hotbar 2",
                3 => "Hotbar 3",
                4 => "Hotbar 4",
                5 => "Hotbar 5",
                6 => "Hotbar 6",
                7 => "Hotbar 7",
                8 => "Hotbar 8",
                _ => "Hotbar 9",
            },
        }
    }

    // Name auf der Steuerungs-Seite: dort steht die Aktion schon in ihrem Abschnitt
    // (CONTROLS_SECTIONS), "Sprachchat: Sprechen" heisst im Abschnitt Sprachchat nur "Sprechen".
    pub(crate) const fn short_label(self) -> &'static str {
        match self {
            Self::PushToTalk => "Sprechen",
            Self::VoiceMute => "Mikrofon stumm",
            Self::VoiceDeafen => "Taub",
            Self::VoiceWhisper => "Flüstern",
            Self::VoiceConfig => "Einstellungen öffnen",
            Self::InventoryMoveStack => "Stapel verschieben",
            Self::InventoryDropItem => "Im Inventar wegwerfen",
            Self::InspectItem => "Item untersuchen",
            Self::InspectHeld => "Item ansehen",
            Self::ToggleHandDebug => "Hand-Debug",
            Self::SaveClip => "Clip",
            Self::SpectatorSlower => "Zuschauer langsamer",
            Self::SpectatorFaster => "Zuschauer schneller",
            Self::Zoom => "Zoom",
            Self::Freelook => "Freelook",
            Self::SwapShoulder => "Schulter wechseln",
            other => other.label(),
        }
    }

    // Stabiler Index in ALL-Reihenfolge -- fuer Arrays wie App::keybind_physical_down, die
    // (anders als Keybinds/KeybindModes) keinen eigenen Satz benannter Felder brauchen.
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Forward => 0,
            Self::Backward => 1,
            Self::Left => 2,
            Self::Right => 3,
            Self::Jump => 4,
            Self::Sneak => 5,
            Self::Sprint => 6,
            Self::TogglePerspective => 7,
            Self::OpenInventory => 8,
            Self::Attack => 9,
            Self::DropItem => 10,
            Self::SwapOffhand => 11,
            Self::OpenChat => 12,
            Self::OpenCommand => 13,
            Self::ToggleDebugOverlay => 14,
            // 15..23 -- Hotbar 1..9
            Self::Hotbar(slot) => 14 + slot.clamp(1, 9) as usize,
            Self::OpenModuleMenu => 24,
            Self::InventoryMoveStack => 25,
            Self::InventoryDropItem => 26,
            Self::Use => 27,
            Self::PlayerList => 28,
            Self::HotbarPrev => 29,
            Self::HotbarNext => 30,
            Self::VolumeUp => 31,
            Self::VolumeDown => 32,
            Self::HideHud => 33,
            Self::ToggleProfiler => 34,
            Self::Zoom => 35,
            Self::PushToTalk => 36,
            Self::InspectItem => 37,
            Self::PickBlock => 42,
            Self::SpectatorSlower => 43,
            Self::SpectatorFaster => 44,
            Self::GameModeWheel => 45,
            Self::Reconnect => 46,
            Self::InspectHeld => 47,
            Self::ToggleHandDebug => 48,
            Self::SaveClip => 51,
            Self::Freelook => 49,
            Self::SwapShoulder => 50,
            Self::VoiceMute => 38,
            Self::VoiceDeafen => 39,
            Self::VoiceWhisper => 40,
            Self::VoiceConfig => 41,
        }
    }

    // Schluesselname in settings.conf ("keybind_<name>" bzw. "keybind_mode_<name>").
    // Die Namen der urspruenglichen sieben Aktionen sind unveraendert, damit bestehende
    // Konfigdateien weiter gelesen werden.
    pub(crate) const fn settings_key(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Backward => "backward",
            Self::Left => "left",
            Self::Right => "right",
            Self::Jump => "jump",
            Self::Sneak => "sneak",
            Self::Sprint => "sprint",
            Self::TogglePerspective => "toggle_perspective",
            Self::OpenInventory => "open_inventory",
            Self::Attack => "attack",
            Self::DropItem => "drop_item",
            Self::SwapOffhand => "swap_offhand",
            Self::OpenChat => "open_chat",
            Self::OpenCommand => "open_command",
            Self::ToggleDebugOverlay => "toggle_debug_overlay",
            Self::OpenModuleMenu => "open_module_menu",
            Self::InventoryMoveStack => "inventory_move_stack",
            Self::InventoryDropItem => "inventory_drop_item",
            Self::Use => "use_item",
            Self::PlayerList => "player_list",
            Self::HotbarPrev => "hotbar_prev",
            Self::HotbarNext => "hotbar_next",
            Self::VolumeUp => "volume_up",
            Self::VolumeDown => "volume_down",
            Self::HideHud => "hide_hud",
            Self::ToggleProfiler => "toggle_profiler",
            Self::Zoom => "zoom",
            Self::Freelook => "freelook",
            Self::SwapShoulder => "swap_shoulder",
            Self::PushToTalk => "push_to_talk",
            Self::InspectItem => "inspect_item",
            Self::InspectHeld => "inspect_held",
            Self::ToggleHandDebug => "toggle_hand_debug",
            Self::SaveClip => "save_clip",
            Self::PickBlock => "pick_block",
            Self::SpectatorSlower => "spectator_slower",
            Self::SpectatorFaster => "spectator_faster",
            Self::GameModeWheel => "gamemode_wheel",
            Self::Reconnect => "reconnect",
            Self::VoiceMute => "voice_mute",
            Self::VoiceDeafen => "voice_deafen",
            Self::VoiceWhisper => "voice_whisper",
            Self::VoiceConfig => "voice_config",
            Self::Hotbar(slot) => match slot {
                1 => "hotbar_1",
                2 => "hotbar_2",
                3 => "hotbar_3",
                4 => "hotbar_4",
                5 => "hotbar_5",
                6 => "hotbar_6",
                7 => "hotbar_7",
                8 => "hotbar_8",
                _ => "hotbar_9",
            },
        }
    }
}

// Aktuell belegte Taste je Aktion. Getrennte Felder statt einer HashMap: die Menge der
// Aktionen aendert sich nur, wenn neue Funktionen dazukommen (seltener Code-Aenderung),
// nicht zur Laufzeit -- ein fester Satz Felder ist dafuer einfacher als eine Map, die jede
// Abfrage mit Option<KeyCode> beantworten muesste.
#[derive(Clone, Copy)]
pub(crate) struct Keybinds {
    // Je Aktion mehrere Eingaben, in fester Slot-Reihenfolge und ohne Luecken (s. remove_at:
    // nach dem Loeschen wird nachgerueckt). Ein Array statt benannter Felder, weil hier jetzt
    // ueber Aktionen UND Slots indiziert wird -- benannte Felder brauchten dafuer zwei
    // ineinandergeschachtelte match-Bloecke.
    pub(crate) slots: [[Option<Bind>; MAX_BINDS_PER_ACTION]; KeyBindAction::ALL.len()],
    // Zweite Taste je Belegung (Modifier, im Fenster "Erweitert" rechts neben ihr): steht eine
    // da, greift die Belegung nur, solange sie zusammen mit ihr unten ist. Gleiche Indizes wie
    // `slots`; zu einem leeren Slot gehoert nie ein Modifier.
    pub(crate) mods: [[Option<Bind>; MAX_BINDS_PER_ACTION]; KeyBindAction::ALL.len()],
}

impl Keybinds {
    // Belegungen einer Aktion samt Modifier, in Slot-Reihenfolge.
    fn pairs(&self, action: KeyBindAction) -> impl Iterator<Item = (Bind, Option<Bind>)> + '_ {
        let i = action.index();
        self.slots[i].iter().zip(self.mods[i]).filter_map(|(b, m)| b.map(|b| (b, m)))
    }

    pub(crate) fn modifier_at(&self, action: KeyBindAction, slot: usize) -> Option<Bind> {
        self.mods[action.index()].get(slot).copied().flatten()
    }

    // Loest der Druck von `bind` diese Aktion aus? Ja, wenn eine Belegung auf ihr liegt und ihr
    // Modifier (falls sie einen hat) gerade unten ist.
    pub(crate) fn matches(&self, action: KeyBindAction, bind: Bind, down: impl Fn(Bind) -> bool) -> bool {
        self.pairs(action).any(|(b, m)| b == bind && m.is_none_or(&down))
    }

    // Ist die Aktion gerade gehalten -- eine Belegung samt Modifier unten?
    pub(crate) fn held(&self, action: KeyBindAction, down: impl Fn(Bind) -> bool) -> bool {
        self.pairs(action).any(|(b, m)| down(b) && m.is_none_or(&down))
    }

    // Wurde mit `released` der Modifier einer Belegung losgelassen, deren Haupttaste noch unten
    // ist? Dann endet die Aktion wie beim Loslassen der Haupttaste.
    pub(crate) fn modifier_released(&self, action: KeyBindAction, released: Bind, down: impl Fn(Bind) -> bool) -> bool {
        self.pairs(action).any(|(b, m)| m == Some(released) && down(b))
    }
    // Alle Eingaben einer Aktion, in Slot-Reihenfolge.
    pub(crate) fn binds(&self, action: KeyBindAction) -> impl Iterator<Item = Bind> + '_ {
        self.slots[action.index()].iter().flatten().copied()
    }

    pub(crate) fn count(&self, action: KeyBindAction) -> usize {
        self.binds(action).count()
    }

    pub(crate) fn bind_at(&self, action: KeyBindAction, slot: usize) -> Option<Bind> {
        self.slots[action.index()].get(slot).copied().flatten()
    }

    // Ist diese Eingabe ueberhaupt irgendeiner Aktion zugeordnet? Der winit-Tastaturpfad
    // braucht das, um zu entscheiden, ob er das Event verbraucht hat oder ob es weiter an
    // Chat/Escape/... gehen soll.
    pub(crate) fn is_bound(&self, bind: Bind) -> bool {
        KeyBindAction::ALL.into_iter().any(|a| self.binds(a).any(|b| b == bind))
    }

    // Belegt einen Slot. Eine Taste darf mehreren Aktionen gehoeren (apply_bind loest dann
    // alle aus, wie Vanilla). Nur innerhalb derselben Aktion faellt eine doppelte Eingabe weg.
    // Eine neue Haupttaste behaelt den Modifier des Slots; doppelt ist nur dieselbe Taste mit
    // demselben Modifier.
    pub(crate) fn set_at(&mut self, action: KeyBindAction, slot: usize, bind: Bind) {
        if slot >= MAX_BINDS_PER_ACTION {
            return;
        }
        let i = action.index();
        let modifier = self.mods[i][slot].filter(|&m| m != bind);
        self.drop_duplicates(action, slot, bind, modifier);
        self.slots[i][slot] = Some(bind);
        self.mods[i][slot] = modifier;
        self.compact(action);
    }

    // Modifier eines belegten Slots setzen oder (None) entfernen. Die Haupttaste selbst kann
    // nicht ihr eigener Modifier sein.
    pub(crate) fn set_modifier_at(&mut self, action: KeyBindAction, slot: usize, modifier: Option<Bind>) {
        let i = action.index();
        let Some(bind) = self.slots[i].get(slot).copied().flatten() else { return };
        if modifier == Some(bind) {
            return;
        }
        self.drop_duplicates(action, slot, bind, modifier);
        self.mods[i][slot] = modifier;
        self.compact(action);
    }

    // Andere Slots der Aktion mit genau dieser Taste und diesem Modifier leeren.
    fn drop_duplicates(&mut self, action: KeyBindAction, slot: usize, bind: Bind, modifier: Option<Bind>) {
        let i = action.index();
        for s in (0..MAX_BINDS_PER_ACTION).filter(|&s| s != slot) {
            if self.slots[i][s] == Some(bind) && self.mods[i][s] == modifier {
                self.slots[i][s] = None;
                self.mods[i][s] = None;
            }
        }
    }

    pub(crate) fn remove_at(&mut self, action: KeyBindAction, slot: usize) {
        if slot < MAX_BINDS_PER_ACTION {
            self.slots[action.index()][slot] = None;
            self.mods[action.index()][slot] = None;
        }
        self.compact(action);
    }

    #[allow(dead_code)] // bleibt fuer den Fall, dass eine Taste wirklich ueberall weg soll
    pub(crate) fn remove_bind_everywhere(&mut self, bind: Bind) {
        for action in KeyBindAction::ALL {
            let i = action.index();
            for s in 0..MAX_BINDS_PER_ACTION {
                if self.slots[i][s] == Some(bind) {
                    self.slots[i][s] = None;
                    self.mods[i][s] = None;
                }
            }
            self.compact(action);
        }
    }

    // Luecken schliessen, damit Slot n+1 nie belegt ist, waehrend Slot n frei ist -- die
    // Tabelle zeichnet die Slots der Reihe nach, ein Loch saehe aus wie ein Fehler.
    pub(crate) fn compact(&mut self, action: KeyBindAction) {
        let i = action.index();
        let (mut slots, mut mods) = ([None; MAX_BINDS_PER_ACTION], [None; MAX_BINDS_PER_ACTION]);
        for (s, (bind, modifier)) in self.pairs(action).enumerate() {
            slots[s] = Some(bind);
            mods[s] = modifier;
        }
        self.slots[i] = slots;
        self.mods[i] = mods;
    }

    // Kein festes Array mehr ueber ALLE Aktionen: die Inventar-Aktionen haben bewusst keine
    // eigene Standardtaste (s. inventory_fallback), die Liste ist also kuerzer als ALL.
    pub(crate) fn from_defaults<const N: usize>(defaults: [(KeyBindAction, Bind); N]) -> Self {
        let mut kb = Self { slots: [[None; MAX_BINDS_PER_ACTION]; KeyBindAction::ALL.len()], mods: [[None; MAX_BINDS_PER_ACTION]; KeyBindAction::ALL.len()] };
        for (action, bind) in defaults {
            kb.slots[action.index()][0] = Some(bind);
        }
        kb
    }
}

impl Default for Keybinds {
    // Vanillas eigene Standardbelegung.
    fn default() -> Self {
        Self::from_defaults([
            (KeyBindAction::Forward, Bind::Key(KeyCode::KeyW)),
            (KeyBindAction::Backward, Bind::Key(KeyCode::KeyS)),
            (KeyBindAction::Left, Bind::Key(KeyCode::KeyA)),
            (KeyBindAction::Right, Bind::Key(KeyCode::KeyD)),
            (KeyBindAction::Jump, Bind::Key(KeyCode::Space)),
            (KeyBindAction::Sneak, Bind::Key(KeyCode::ShiftLeft)),
            (KeyBindAction::Sprint, Bind::Key(KeyCode::ControlLeft)),
            (KeyBindAction::TogglePerspective, Bind::Key(KeyCode::F5)),
            (KeyBindAction::OpenInventory, Bind::Key(KeyCode::KeyE)),
            (KeyBindAction::Attack, Bind::Mouse(BindMouse::Left)),
            (KeyBindAction::Use, Bind::Mouse(BindMouse::Right)),
            (KeyBindAction::PlayerList, Bind::Key(KeyCode::Tab)),
            (KeyBindAction::DropItem, Bind::Key(KeyCode::KeyQ)),
            (KeyBindAction::SwapOffhand, Bind::Key(KeyCode::KeyF)),
            (KeyBindAction::OpenChat, Bind::Key(KeyCode::KeyT)),
            (KeyBindAction::OpenCommand, Bind::Key(KeyCode::Slash)),
            (KeyBindAction::ToggleDebugOverlay, Bind::Key(KeyCode::F3)),
            (KeyBindAction::HideHud, Bind::Key(KeyCode::F1)),
            (KeyBindAction::ToggleProfiler, Bind::Key(KeyCode::F6)),
            (KeyBindAction::GameModeWheel, Bind::Key(KeyCode::F4)),
            // Wie osus Neustart-Taste: ^ auf deutscher Tastatur (physisch die Backquote-Taste).
            (KeyBindAction::Reconnect, Bind::Key(KeyCode::Backquote)),
            // Wie bei den bekannten Zoom-Mods (OptiFine, Zoomify).
            (KeyBindAction::Zoom, Bind::Key(KeyCode::KeyC)),
            // Wie bei Simple Voice Chat.
            (KeyBindAction::PushToTalk, Bind::Key(KeyCode::CapsLock)),
            // Wie die erweiterten Tooltips anderer Clients: mit dem Daumen erreichbar.
            (KeyBindAction::InspectItem, Bind::Key(KeyCode::AltLeft)),
            (KeyBindAction::PickBlock, Bind::Mouse(BindMouse::Middle)),
            // Bei Vanilla unbelegt, neben F (Inspect in Counter-Strike; F tauscht hier die Haende).
            (KeyBindAction::InspectHeld, Bind::Key(KeyCode::KeyG)),
            // Bei Vanilla unbelegt, neben dem Frame-Profiler (F6).
            (KeyBindAction::ToggleHandDebug, Bind::Key(KeyCode::F7)),
            // Bei Vanilla unbelegt, neben dem Hand-Debug (F7).
            (KeyBindAction::SaveClip, Bind::Key(KeyCode::F8)),
            // Wie bei Simple Voice Chat: M stumm, N taub; Fluestern ab Werk unbelegt.
            (KeyBindAction::VoiceMute, Bind::Key(KeyCode::KeyM)),
            (KeyBindAction::VoiceDeafen, Bind::Key(KeyCode::KeyN)),
            // V oeffnet bei der Mod ihr Menue.
            (KeyBindAction::VoiceConfig, Bind::Key(KeyCode::KeyV)),
            // Rechte Shift-Taste: bei Vanilla unbelegt, in der Naehe der rechten Hand und
            // nicht mit dem Schleichen (linkes Shift) verwechselbar.
            (KeyBindAction::OpenModuleMenu, Bind::Key(KeyCode::ShiftRight)),
            (KeyBindAction::Hotbar(1), Bind::Key(KeyCode::Digit1)),
            (KeyBindAction::Hotbar(2), Bind::Key(KeyCode::Digit2)),
            (KeyBindAction::Hotbar(3), Bind::Key(KeyCode::Digit3)),
            (KeyBindAction::Hotbar(4), Bind::Key(KeyCode::Digit4)),
            (KeyBindAction::Hotbar(5), Bind::Key(KeyCode::Digit5)),
            (KeyBindAction::Hotbar(6), Bind::Key(KeyCode::Digit6)),
            (KeyBindAction::Hotbar(7), Bind::Key(KeyCode::Digit7)),
            (KeyBindAction::Hotbar(8), Bind::Key(KeyCode::Digit8)),
            (KeyBindAction::Hotbar(9), Bind::Key(KeyCode::Digit9)),
            // Wie Vanilla: Rad hoch = ein Platz nach links, Rad runter = nach rechts.
            (KeyBindAction::HotbarPrev, Bind::Mouse(BindMouse::WheelUp)),
            (KeyBindAction::HotbarNext, Bind::Mouse(BindMouse::WheelDown)),
            // Die beiden Inventar-Aktionen bekommen bewusst KEINE eigene Taste: ohne
            // Belegung gilt die ihres Welt-Gegenstuecks (s. inventory_fallback), also Shift
            // und Q wie in Vanilla. Erst wer hier etwas eintraegt, trennt die beiden.
        ])
    }
}

// Halten (wie Vanilla es fuer Bewegungstasten kennt) vs. Umschalten (ein Tastendruck
// schaltet den Zustand um, s. apply_key_action). Vanilla selbst bietet das nur fuer
// Schleichen/Sprinten an -- hier bewusst fuer JEDE Taste der Tabelle (Nutzerwunsch), auch
// wenn "Vorwaerts umschalten" kein Vanilla-Vorbild hat.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeybindMode {
    Hold,
    Toggle,
}

impl KeybindMode {
    // Kaestchen neben der Belegung: H(alten) oder T(oggle).
    pub(crate) const fn short(self) -> &'static str {
        match self {
            Self::Hold => "H",
            Self::Toggle => "T",
        }
    }

    pub(crate) const fn toggled(self) -> Self {
        match self {
            Self::Hold => Self::Toggle,
            Self::Toggle => Self::Hold,
        }
    }
}

// Modus je Aktion -- gleiches Muster wie Keybinds (feste Felder statt Map, s. dortigen
// Kommentar).
#[derive(Clone, Copy)]
pub(crate) struct KeybindModes {
    pub(crate) forward: KeybindMode,
    pub(crate) backward: KeybindMode,
    pub(crate) left: KeybindMode,
    pub(crate) right: KeybindMode,
    pub(crate) jump: KeybindMode,
    pub(crate) sneak: KeybindMode,
    pub(crate) sprint: KeybindMode,
    // Umschalten ist hier keine Spielerei: gehaltener Rechtsklick platziert dauerhaft, und
    // wer das umschaltbar will, braucht die Taste nicht festzuhalten.
    pub(crate) use_item: KeybindMode,
    pub(crate) player_list: KeybindMode,
    pub(crate) zoom: KeybindMode,
    pub(crate) freelook: KeybindMode,
    pub(crate) push_to_talk: KeybindMode,
    pub(crate) inspect_item: KeybindMode,
    pub(crate) voice_whisper: KeybindMode,
    pub(crate) perspective: KeybindMode,
}

impl KeybindModes {
    pub(crate) const fn get(&self, action: KeyBindAction) -> KeybindMode {
        match action {
            KeyBindAction::Forward => self.forward,
            KeyBindAction::Backward => self.backward,
            KeyBindAction::Left => self.left,
            KeyBindAction::Right => self.right,
            KeyBindAction::Jump => self.jump,
            KeyBindAction::Sneak => self.sneak,
            KeyBindAction::Sprint => self.sprint,
            KeyBindAction::Use => self.use_item,
            KeyBindAction::PlayerList => self.player_list,
            KeyBindAction::Zoom => self.zoom,
            KeyBindAction::Freelook => self.freelook,
            KeyBindAction::PushToTalk => self.push_to_talk,
            KeyBindAction::InspectItem => self.inspect_item,
            KeyBindAction::VoiceWhisper => self.voice_whisper,
            KeyBindAction::TogglePerspective => self.perspective,
            // Einmal-Aktionen haben keinen Modus (s. is_press_action) -- Hold ist hier
            // schlicht der neutrale Wert, benutzt wird er nie.
            // Einmal-Aktionen haben keinen Halte-/Umschalt-Zustand.
            _ => KeybindMode::Hold,
        }
    }

    pub(crate) const fn set(&mut self, action: KeyBindAction, mode: KeybindMode) {
        match action {
            KeyBindAction::Forward => self.forward = mode,
            KeyBindAction::Backward => self.backward = mode,
            KeyBindAction::Left => self.left = mode,
            KeyBindAction::Right => self.right = mode,
            KeyBindAction::Jump => self.jump = mode,
            KeyBindAction::Sneak => self.sneak = mode,
            KeyBindAction::Sprint => self.sprint = mode,
            KeyBindAction::Use => self.use_item = mode,
            KeyBindAction::PlayerList => self.player_list = mode,
            KeyBindAction::Zoom => self.zoom = mode,
            KeyBindAction::Freelook => self.freelook = mode,
            KeyBindAction::PushToTalk => self.push_to_talk = mode,
            KeyBindAction::InspectItem => self.inspect_item = mode,
            KeyBindAction::VoiceWhisper => self.voice_whisper = mode,
            KeyBindAction::TogglePerspective => self.perspective = mode,
            _ => {}
        }
    }
}

impl Default for KeybindModes {
    // Wie Vanilla per Default: alles Halten.
    fn default() -> Self {
        Self {
            forward: KeybindMode::Hold,
            backward: KeybindMode::Hold,
            left: KeybindMode::Hold,
            right: KeybindMode::Hold,
            jump: KeybindMode::Hold,
            sneak: KeybindMode::Hold,
            sprint: KeybindMode::Hold,
            use_item: KeybindMode::Hold,
            player_list: KeybindMode::Hold,
            zoom: KeybindMode::Hold,
            freelook: KeybindMode::Hold,
            push_to_talk: KeybindMode::Hold,
            inspect_item: KeybindMode::Hold,
            voice_whisper: KeybindMode::Hold,
            // Umschalten wie Vanillas F5; Halten (Quicklook) nur auf Wunsch.
            perspective: KeybindMode::Toggle,
        }
    }
}

pub(crate) fn keybind_mode_from_str(value: &str) -> Option<KeybindMode> {
    match value {
        "hold" => Some(KeybindMode::Hold),
        "toggle" => Some(KeybindMode::Toggle),
        _ => None,
    }
}

pub(crate) const fn keybind_mode_to_str(mode: KeybindMode) -> &'static str {
    match mode {
        KeybindMode::Hold => "hold",
        KeybindMode::Toggle => "toggle",
    }
}

// KeyCode ist #[non_exhaustive] und implementiert kein FromStr/Display -- Hin- und
// Rueckrichtung fuer die Settings-Datei deshalb von Hand. Die Hin-Richtung kann
// Debug-Formatierung nutzen (die Variantennamen wie "KeyW" passen 1:1), nur die
// Rueckrichtung braucht ein explizites Match. Deckt den gesamten KeyCode-Katalog ab, nicht
// nur die aktuell belegbaren Aktionen -- ein Nutzer koennte jede Taste eintragen wollen.
// Eine belegbare Eingabe: Tastaturtaste ODER Maustaste. Frueher stand hier ueberall direkt
// ein KeyCode -- damit liessen sich Maustasten prinzipiell nicht belegen, obwohl gerade sie
// (Seitentasten fuer Sprint/Schleichen) im PvP ueblich sind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Bind {
    Key(KeyCode),
    Mouse(BindMouse),
}

// Eigener Maustasten-Typ statt winits: er muss auch die Tasten aus dem evdev-Raw-Pfad
// abbilden (s. raw_input::MouseButton) und in settings.conf einen stabilen Namen haben.
// winit nennt die Daumentasten Back/Forward, evdev Side/Extra -- gemeint sind dieselben.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BindMouse {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
    // Das Mausrad, je Richtung eine eigene "Taste". Es kennt kein Loslassen: jede Raste ist
    // ein Druck mit sofortigem Loslassen (s. App::wheel_notch).
    WheelUp,
    WheelDown,
}

impl BindMouse {
    // Nummer fuer Addons (s. coral_addon::Event::Mouse).
    pub(crate) const fn addon_index(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Middle => 2,
            Self::Back => 3,
            Self::Forward => 4,
            Self::Other(n) => (n as u8).saturating_add(5),
            Self::WheelUp | Self::WheelDown => u8::MAX,
        }
    }

    pub(crate) const fn from_winit(button: MouseButton) -> Option<Self> {
        Some(match button {
            MouseButton::Left => Self::Left,
            MouseButton::Right => Self::Right,
            MouseButton::Middle => Self::Middle,
            MouseButton::Back => Self::Back,
            MouseButton::Forward => Self::Forward,
            MouseButton::Other(n) => Self::Other(n),
        })
    }

    pub(crate) const fn from_raw(button: raw_input::MouseButton) -> Self {
        match button {
            raw_input::MouseButton::Left => Self::Left,
            raw_input::MouseButton::Right => Self::Right,
            raw_input::MouseButton::Middle => Self::Middle,
            raw_input::MouseButton::Side => Self::Back,
            raw_input::MouseButton::Extra => Self::Forward,
        }
    }

    // Name fuer Anzeige UND settings.conf -- bewusst derselbe, damit es keine zweite,
    // auseinanderlaufende Tabelle gibt (wie schon bei den Tasten, s. keycode_name).
    pub(crate) fn name(self) -> String {
        match self {
            Self::Left => "Maus links".to_string(),
            Self::Right => "Maus rechts".to_string(),
            Self::Middle => "Maus mitte".to_string(),
            Self::Back => "Maus zurück".to_string(),
            Self::Forward => "Maus vor".to_string(),
            Self::Other(n) => format!("Maus {n}"),
            Self::WheelUp => "Mausrad hoch".to_string(),
            Self::WheelDown => "Mausrad runter".to_string(),
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "Maus links" => Self::Left,
            "Maus rechts" => Self::Right,
            "Maus mitte" => Self::Middle,
            // Alte Einstellungsdateien schrieben noch ohne Umlaut.
            "Maus zurück" | "Maus zurueck" => Self::Back,
            "Maus vor" => Self::Forward,
            "Mausrad hoch" => Self::WheelUp,
            "Mausrad runter" => Self::WheelDown,
            other => Self::Other(other.strip_prefix("Maus ")?.parse().ok()?),
        })
    }
}

// Fenster "Erweitert" (s. App::keybind_advanced_layout), Bildschirmkoordinaten.
pub(crate) struct KeybindAdvancedLayout {
    pub(crate) window: crate::Rect,
    // Je Belegung ihr Feld und rechts daneben das ihres Modifiers.
    pub(crate) binds: Vec<crate::Rect>,
    pub(crate) modifiers: Vec<crate::Rect>,
    // "+" fuer eine weitere Belegung; None, wenn alle Plaetze voll sind.
    pub(crate) plus: Option<crate::Rect>,
    pub(crate) done: crate::Rect,
}

// Wie viele Eingaben eine Aktion tragen kann. Die Seite zeigt je Aktion nur die erste, alle
// stehen im Fenster "Erweitert" (s. build_keybind_advanced).
pub(crate) const MAX_BINDS_PER_ACTION: usize = 5;

// Name NUR fuer die Anzeige: Maustasten durchnummeriert wie auf den Maeusen selbst (1 links,
// 2 rechts, 3 Rad, 4 und 5 die Seitentasten). Gespeichert wird weiterhin der ausgeschriebene
// Name (s. BindMouse::name), damit alte settings.conf gueltig bleiben.
pub(crate) fn bind_label(bind: Bind) -> String {
    match bind {
        Bind::Key(code) => keycode_name(code),
        Bind::Mouse(BindMouse::Left) => "Maustaste 1".to_string(),
        Bind::Mouse(BindMouse::Right) => "Maustaste 2".to_string(),
        Bind::Mouse(BindMouse::Middle) => "Maustaste 3".to_string(),
        Bind::Mouse(BindMouse::Back) => "Maustaste 4".to_string(),
        Bind::Mouse(BindMouse::Forward) => "Maustaste 5".to_string(),
        Bind::Mouse(BindMouse::WheelUp) => "Mausrad hoch".to_string(),
        Bind::Mouse(BindMouse::WheelDown) => "Mausrad runter".to_string(),
        Bind::Mouse(BindMouse::Other(n)) => format!("Maustaste {n}"),
    }
}

// Lesbarer Name fuer die Anzeige: "A" statt "KeyA", "Strg links" statt "ControlLeft".
// Satzzeichen-Tasten nach deutscher Belegung beschriftet (KeyCode ist die physische Lage, die
// Bezeichnung auf der Taste haengt am Layout).
// Eine Belegung zur Anzeige, mit ihrem Modifier davor: "Strg links + E".
pub(crate) fn slot_display(kb: &Keybinds, action: KeyBindAction, slot: usize) -> Option<String> {
    let bind = kb.bind_at(action, slot)?;
    Some(match kb.modifier_at(action, slot) {
        Some(m) => format!("{} + {}", bind_display(m), bind_display(bind)),
        None => bind_display(bind),
    })
}

pub(crate) fn bind_display(bind: Bind) -> String {
    let Bind::Key(code) = bind else { return bind_label(bind) };
    let name = keycode_name(code);
    if let Some(rest) = name.strip_prefix("Key").or_else(|| name.strip_prefix("Digit")) {
        return rest.to_string();
    }
    if let Some(rest) = name.strip_prefix("Numpad") {
        let zeichen = match rest {
            "Add" => "+",
            "Subtract" => "-",
            "Multiply" => "*",
            "Divide" => "/",
            "Decimal" => ",",
            "Enter" => "Enter",
            "Equal" => "=",
            other => other,
        };
        return format!("Num {zeichen}");
    }
    match code {
        KeyCode::Space => "Leertaste",
        KeyCode::ShiftLeft => "Shift links",
        KeyCode::ShiftRight => "Shift rechts",
        KeyCode::ControlLeft => "Strg links",
        KeyCode::ControlRight => "Strg rechts",
        KeyCode::AltLeft => "Alt",
        KeyCode::AltRight => "Alt Gr",
        KeyCode::SuperLeft | KeyCode::SuperRight => "Super",
        KeyCode::CapsLock => "Feststell",
        KeyCode::Tab => "Tab",
        KeyCode::Enter => "Enter",
        KeyCode::Backspace => "Rücktaste",
        KeyCode::Escape => "Esc",
        KeyCode::Insert => "Einfg",
        KeyCode::Delete => "Entf",
        KeyCode::Home => "Pos1",
        KeyCode::End => "Ende",
        KeyCode::PageUp => "Bild hoch",
        KeyCode::PageDown => "Bild runter",
        KeyCode::ArrowUp => "Pfeil hoch",
        KeyCode::ArrowDown => "Pfeil runter",
        KeyCode::ArrowLeft => "Pfeil links",
        KeyCode::ArrowRight => "Pfeil rechts",
        KeyCode::Backquote => "^",
        KeyCode::Minus => "ß",
        KeyCode::Equal => "´",
        KeyCode::BracketLeft => "Ü",
        KeyCode::BracketRight => "+",
        KeyCode::Semicolon => "Ö",
        KeyCode::Quote => "Ä",
        KeyCode::Backslash => "#",
        KeyCode::IntlBackslash => "<",
        KeyCode::Comma => ",",
        KeyCode::Period => ".",
        KeyCode::Slash => "-",
        KeyCode::PrintScreen => "Druck",
        KeyCode::ScrollLock => "Rollen",
        KeyCode::Pause => "Pause",
        KeyCode::NumLock => "Num",
        KeyCode::ContextMenu => "Menü",
        _ => return name,
    }
    .to_string()
}

pub(crate) fn bind_name(bind: Bind) -> String {
    match bind {
        Bind::Key(code) => keycode_name(code),
        Bind::Mouse(button) => button.name(),
    }
}

// Erst Maustaste, dann Taste -- die Namensraeume ueberschneiden sich nicht ("Maus ..." kommt
// als KeyCode-Name nie vor). Eine settings.conf aus der Zeit vor den Maustasten enthaelt
// ausschliesslich KeyCode-Namen und wird deshalb unveraendert weiter gelesen.
pub(crate) fn bind_from_name(name: &str) -> Option<Bind> {
    if let Some(button) = BindMouse::from_name(name) {
        return Some(Bind::Mouse(button));
    }
    keycode_from_name(name).map(Bind::Key)
}

pub(crate) fn keycode_name(code: KeyCode) -> String {
    format!("{code:?}")
}

pub(crate) fn keycode_from_name(name: &str) -> Option<KeyCode> {
    match name {
        "Backquote" => Some(KeyCode::Backquote),
        "Backslash" => Some(KeyCode::Backslash),
        "BracketLeft" => Some(KeyCode::BracketLeft),
        "BracketRight" => Some(KeyCode::BracketRight),
        "Comma" => Some(KeyCode::Comma),
        "Digit0" => Some(KeyCode::Digit0),
        "Digit1" => Some(KeyCode::Digit1),
        "Digit2" => Some(KeyCode::Digit2),
        "Digit3" => Some(KeyCode::Digit3),
        "Digit4" => Some(KeyCode::Digit4),
        "Digit5" => Some(KeyCode::Digit5),
        "Digit6" => Some(KeyCode::Digit6),
        "Digit7" => Some(KeyCode::Digit7),
        "Digit8" => Some(KeyCode::Digit8),
        "Digit9" => Some(KeyCode::Digit9),
        "Equal" => Some(KeyCode::Equal),
        "IntlBackslash" => Some(KeyCode::IntlBackslash),
        "IntlRo" => Some(KeyCode::IntlRo),
        "IntlYen" => Some(KeyCode::IntlYen),
        "KeyA" => Some(KeyCode::KeyA),
        "KeyB" => Some(KeyCode::KeyB),
        "KeyC" => Some(KeyCode::KeyC),
        "KeyD" => Some(KeyCode::KeyD),
        "KeyE" => Some(KeyCode::KeyE),
        "KeyF" => Some(KeyCode::KeyF),
        "KeyG" => Some(KeyCode::KeyG),
        "KeyH" => Some(KeyCode::KeyH),
        "KeyI" => Some(KeyCode::KeyI),
        "KeyJ" => Some(KeyCode::KeyJ),
        "KeyK" => Some(KeyCode::KeyK),
        "KeyL" => Some(KeyCode::KeyL),
        "KeyM" => Some(KeyCode::KeyM),
        "KeyN" => Some(KeyCode::KeyN),
        "KeyO" => Some(KeyCode::KeyO),
        "KeyP" => Some(KeyCode::KeyP),
        "KeyQ" => Some(KeyCode::KeyQ),
        "KeyR" => Some(KeyCode::KeyR),
        "KeyS" => Some(KeyCode::KeyS),
        "KeyT" => Some(KeyCode::KeyT),
        "KeyU" => Some(KeyCode::KeyU),
        "KeyV" => Some(KeyCode::KeyV),
        "KeyW" => Some(KeyCode::KeyW),
        "KeyX" => Some(KeyCode::KeyX),
        "KeyY" => Some(KeyCode::KeyY),
        "KeyZ" => Some(KeyCode::KeyZ),
        "Minus" => Some(KeyCode::Minus),
        "Period" => Some(KeyCode::Period),
        "Quote" => Some(KeyCode::Quote),
        "Semicolon" => Some(KeyCode::Semicolon),
        "Slash" => Some(KeyCode::Slash),
        "AltLeft" => Some(KeyCode::AltLeft),
        "AltRight" => Some(KeyCode::AltRight),
        "Backspace" => Some(KeyCode::Backspace),
        "CapsLock" => Some(KeyCode::CapsLock),
        "ContextMenu" => Some(KeyCode::ContextMenu),
        "ControlLeft" => Some(KeyCode::ControlLeft),
        "ControlRight" => Some(KeyCode::ControlRight),
        "Enter" => Some(KeyCode::Enter),
        "SuperLeft" => Some(KeyCode::SuperLeft),
        "SuperRight" => Some(KeyCode::SuperRight),
        "ShiftLeft" => Some(KeyCode::ShiftLeft),
        "ShiftRight" => Some(KeyCode::ShiftRight),
        "Space" => Some(KeyCode::Space),
        "Tab" => Some(KeyCode::Tab),
        "Convert" => Some(KeyCode::Convert),
        "KanaMode" => Some(KeyCode::KanaMode),
        "Lang1" => Some(KeyCode::Lang1),
        "Lang2" => Some(KeyCode::Lang2),
        "Lang3" => Some(KeyCode::Lang3),
        "Lang4" => Some(KeyCode::Lang4),
        "Lang5" => Some(KeyCode::Lang5),
        "NonConvert" => Some(KeyCode::NonConvert),
        "Delete" => Some(KeyCode::Delete),
        "End" => Some(KeyCode::End),
        "Help" => Some(KeyCode::Help),
        "Home" => Some(KeyCode::Home),
        "Insert" => Some(KeyCode::Insert),
        "PageDown" => Some(KeyCode::PageDown),
        "PageUp" => Some(KeyCode::PageUp),
        "ArrowDown" => Some(KeyCode::ArrowDown),
        "ArrowLeft" => Some(KeyCode::ArrowLeft),
        "ArrowRight" => Some(KeyCode::ArrowRight),
        "ArrowUp" => Some(KeyCode::ArrowUp),
        "NumLock" => Some(KeyCode::NumLock),
        "Numpad0" => Some(KeyCode::Numpad0),
        "Numpad1" => Some(KeyCode::Numpad1),
        "Numpad2" => Some(KeyCode::Numpad2),
        "Numpad3" => Some(KeyCode::Numpad3),
        "Numpad4" => Some(KeyCode::Numpad4),
        "Numpad5" => Some(KeyCode::Numpad5),
        "Numpad6" => Some(KeyCode::Numpad6),
        "Numpad7" => Some(KeyCode::Numpad7),
        "Numpad8" => Some(KeyCode::Numpad8),
        "Numpad9" => Some(KeyCode::Numpad9),
        "NumpadAdd" => Some(KeyCode::NumpadAdd),
        "NumpadBackspace" => Some(KeyCode::NumpadBackspace),
        "NumpadClear" => Some(KeyCode::NumpadClear),
        "NumpadClearEntry" => Some(KeyCode::NumpadClearEntry),
        "NumpadComma" => Some(KeyCode::NumpadComma),
        "NumpadDecimal" => Some(KeyCode::NumpadDecimal),
        "NumpadDivide" => Some(KeyCode::NumpadDivide),
        "NumpadEnter" => Some(KeyCode::NumpadEnter),
        "NumpadEqual" => Some(KeyCode::NumpadEqual),
        "NumpadHash" => Some(KeyCode::NumpadHash),
        "NumpadMemoryAdd" => Some(KeyCode::NumpadMemoryAdd),
        "NumpadMemoryClear" => Some(KeyCode::NumpadMemoryClear),
        "NumpadMemoryRecall" => Some(KeyCode::NumpadMemoryRecall),
        "NumpadMemoryStore" => Some(KeyCode::NumpadMemoryStore),
        "NumpadMemorySubtract" => Some(KeyCode::NumpadMemorySubtract),
        "NumpadMultiply" => Some(KeyCode::NumpadMultiply),
        "NumpadParenLeft" => Some(KeyCode::NumpadParenLeft),
        "NumpadParenRight" => Some(KeyCode::NumpadParenRight),
        "NumpadStar" => Some(KeyCode::NumpadStar),
        "NumpadSubtract" => Some(KeyCode::NumpadSubtract),
        "Escape" => Some(KeyCode::Escape),
        "Fn" => Some(KeyCode::Fn),
        "FnLock" => Some(KeyCode::FnLock),
        "PrintScreen" => Some(KeyCode::PrintScreen),
        "ScrollLock" => Some(KeyCode::ScrollLock),
        "Pause" => Some(KeyCode::Pause),
        "BrowserBack" => Some(KeyCode::BrowserBack),
        "BrowserFavorites" => Some(KeyCode::BrowserFavorites),
        "BrowserForward" => Some(KeyCode::BrowserForward),
        "BrowserHome" => Some(KeyCode::BrowserHome),
        "BrowserRefresh" => Some(KeyCode::BrowserRefresh),
        "BrowserSearch" => Some(KeyCode::BrowserSearch),
        "BrowserStop" => Some(KeyCode::BrowserStop),
        "Eject" => Some(KeyCode::Eject),
        "LaunchApp1" => Some(KeyCode::LaunchApp1),
        "LaunchApp2" => Some(KeyCode::LaunchApp2),
        "LaunchMail" => Some(KeyCode::LaunchMail),
        "MediaPlayPause" => Some(KeyCode::MediaPlayPause),
        "MediaSelect" => Some(KeyCode::MediaSelect),
        "MediaStop" => Some(KeyCode::MediaStop),
        "MediaTrackNext" => Some(KeyCode::MediaTrackNext),
        "MediaTrackPrevious" => Some(KeyCode::MediaTrackPrevious),
        "Power" => Some(KeyCode::Power),
        "Sleep" => Some(KeyCode::Sleep),
        "AudioVolumeDown" => Some(KeyCode::AudioVolumeDown),
        "AudioVolumeMute" => Some(KeyCode::AudioVolumeMute),
        "AudioVolumeUp" => Some(KeyCode::AudioVolumeUp),
        "WakeUp" => Some(KeyCode::WakeUp),
        "Meta" => Some(KeyCode::Meta),
        "Hyper" => Some(KeyCode::Hyper),
        "Turbo" => Some(KeyCode::Turbo),
        "Abort" => Some(KeyCode::Abort),
        "Resume" => Some(KeyCode::Resume),
        "Suspend" => Some(KeyCode::Suspend),
        "Again" => Some(KeyCode::Again),
        "Copy" => Some(KeyCode::Copy),
        "Cut" => Some(KeyCode::Cut),
        "Find" => Some(KeyCode::Find),
        "Open" => Some(KeyCode::Open),
        "Paste" => Some(KeyCode::Paste),
        "Props" => Some(KeyCode::Props),
        "Select" => Some(KeyCode::Select),
        "Undo" => Some(KeyCode::Undo),
        "Hiragana" => Some(KeyCode::Hiragana),
        "Katakana" => Some(KeyCode::Katakana),
        "F1" => Some(KeyCode::F1),
        "F2" => Some(KeyCode::F2),
        "F3" => Some(KeyCode::F3),
        "F4" => Some(KeyCode::F4),
        "F5" => Some(KeyCode::F5),
        "F6" => Some(KeyCode::F6),
        "F7" => Some(KeyCode::F7),
        "F8" => Some(KeyCode::F8),
        "F9" => Some(KeyCode::F9),
        "F10" => Some(KeyCode::F10),
        "F11" => Some(KeyCode::F11),
        "F12" => Some(KeyCode::F12),
        "F13" => Some(KeyCode::F13),
        "F14" => Some(KeyCode::F14),
        "F15" => Some(KeyCode::F15),
        "F16" => Some(KeyCode::F16),
        "F17" => Some(KeyCode::F17),
        "F18" => Some(KeyCode::F18),
        "F19" => Some(KeyCode::F19),
        "F20" => Some(KeyCode::F20),
        "F21" => Some(KeyCode::F21),
        "F22" => Some(KeyCode::F22),
        "F23" => Some(KeyCode::F23),
        "F24" => Some(KeyCode::F24),
        "F25" => Some(KeyCode::F25),
        "F26" => Some(KeyCode::F26),
        "F27" => Some(KeyCode::F27),
        "F28" => Some(KeyCode::F28),
        "F29" => Some(KeyCode::F29),
        "F30" => Some(KeyCode::F30),
        "F31" => Some(KeyCode::F31),
        "F32" => Some(KeyCode::F32),
        "F33" => Some(KeyCode::F33),
        "F34" => Some(KeyCode::F34),
        "F35" => Some(KeyCode::F35),
        _ => None,
    }
}
