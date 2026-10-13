// Toteming Pro Max -- die Fabric-Mod des Nutzers (~/Schreibtisch/toteming pro max),
// nachgebaut fuer diesen Client. Vier Module im Modul-Menue, je eins pro Bereich der Mod:
//
// - Totem-Warnung: zwei X neben dem Fadenkreuz (TotemWarningHud). Zweithand-Seite = kein
//   Totem in der Zweithand, Haupthand-Seite = kein Totem in der Hotbar.
// - Totem-Vorschau: die naechstgelegenen Totems schon VOR dem Oeffnen des Inventars an der
//   Stelle einblenden, an der ihr Slot gleich liegen wird (TotemSlotOverlayHud).
// - Pop-Klaenge: fuenf belegbare Klangplaetze (TotemPopSound + SoundReplacements).
// - Pop-Optik: Flug-Totem und Partikelwolken abschaltbar (ClientPlayNetworkHandlerMixin).
//
// Die Pop-Animation selbst (Vanillas schwebendes Totem) steckte schon vorher im Client
// (s. App::emit_floating_totem), der Pop-Zaehler (TotemPopTracker) schreibt ins Log.
//
// Bewusste Abweichungen von der Mod:
// - Keine feste/gemessene GUI-Skalierung fuer die Vorschau (overlayGuiScale +
//   InventoryGeometry). Die Mod musste raten, wo eine ANDERE Mod das Inventar hinlegt --
//   hier ist es derselbe Code, der das Inventar zeichnet (inventory::player_screen_layout).
// - Kein Schalter "eigener Pop am Ohr". Der eigene Pop kommt immer kopfgebunden: er wird im
//   Netzwerk-Task gespielt (Sofort-Haken, s. App::start_connect), und die eigene Position
//   liegt direkt unter den Ohren -- im Ohrenmodell klingt beides gleich, nur spaeter.
// - Eigene Klangdateien liegen in ~/.config/coral-client/sounds/ (.ogg oder .wav) statt im
//   Fabric-Config-Ordner.

use std::collections::HashMap;
use std::sync::Arc;

use audio::wav::Wav;

use crate::inventory::SlotRect;

// --- Farben der X ---------------------------------------------------------------------

// Feste Auswahl wie in der Mod (IndicatorColor): ein Stepper ist im Spiel schneller bedient
// als ein Farbeditor, und jede dieser Farben ist vor Tag- und Nachthimmel lesbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndicatorColor {
    White,
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    LightBlue,
    Purple,
    Pink,
    Black,
}

impl IndicatorColor {
    pub const ALL: [IndicatorColor; 10] = [
        IndicatorColor::White,
        IndicatorColor::Red,
        IndicatorColor::Orange,
        IndicatorColor::Yellow,
        IndicatorColor::Green,
        IndicatorColor::Cyan,
        IndicatorColor::LightBlue,
        IndicatorColor::Purple,
        IndicatorColor::Pink,
        IndicatorColor::Black,
    ];

    // ARGB als sRGB, genau die Werte der Mod.
    pub fn argb(self) -> u32 {
        match self {
            IndicatorColor::White => 0xFFFF_FFFF,
            IndicatorColor::Red => 0xFFFF_0000,
            IndicatorColor::Orange => 0xFFFF_8000,
            IndicatorColor::Yellow => 0xFFFF_FF00,
            IndicatorColor::Green => 0xFF00_FF40,
            IndicatorColor::Cyan => 0xFF00_FFFF,
            IndicatorColor::LightBlue => 0xFF00_80FF,
            IndicatorColor::Purple => 0xFFB0_40FF,
            IndicatorColor::Pink => 0xFFFF_40C0,
            IndicatorColor::Black => 0xFF00_0000,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            IndicatorColor::White => "Weiß",
            IndicatorColor::Red => "Rot",
            IndicatorColor::Orange => "Orange",
            IndicatorColor::Yellow => "Gelb",
            IndicatorColor::Green => "Grün",
            IndicatorColor::Cyan => "Cyan",
            IndicatorColor::LightBlue => "Hellblau",
            IndicatorColor::Purple => "Lila",
            IndicatorColor::Pink => "Pink",
            IndicatorColor::Black => "Schwarz",
        }
    }

    fn key(self) -> &'static str {
        match self {
            IndicatorColor::White => "white",
            IndicatorColor::Red => "red",
            IndicatorColor::Orange => "orange",
            IndicatorColor::Yellow => "yellow",
            IndicatorColor::Green => "green",
            IndicatorColor::Cyan => "cyan",
            IndicatorColor::LightBlue => "light_blue",
            IndicatorColor::Purple => "purple",
            IndicatorColor::Pink => "pink",
            IndicatorColor::Black => "black",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }

    // Laeuft um: nach Schwarz kommt wieder Weiss.
    fn step(self, dir: i8) -> Self {
        let i = Self::ALL.iter().position(|c| *c == self).unwrap_or(0) as i32;
        Self::ALL[(i + dir as i32).rem_euclid(Self::ALL.len() as i32) as usize]
    }
}

// --- Untergrund der Vorschau ----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    // Versetzter dunkler Fleck ohne Rahmen -- sieht nach Schatten aus.
    Shadow,
    // Dunkles Kaestchen mit hellem Rahmen.
    Box,
    None,
}

impl Backdrop {
    const ALL: [Backdrop; 3] = [Backdrop::Shadow, Backdrop::Box, Backdrop::None];

    pub fn label(self) -> &'static str {
        match self {
            Backdrop::Shadow => "Schatten",
            Backdrop::Box => "Kasten",
            Backdrop::None => "Keiner",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Backdrop::Shadow => "shadow",
            Backdrop::Box => "box",
            Backdrop::None => "none",
        }
    }

    fn step(self, dir: i8) -> Self {
        let i = Self::ALL.iter().position(|b| *b == self).unwrap_or(0) as i32;
        Self::ALL[(i + dir as i32).rem_euclid(Self::ALL.len() as i32) as usize]
    }
}

// --- Klangplaetze ---------------------------------------------------------------------

// Sonderwerte statt eines Klangs (SoundSetting.VANILLA/OFF).
pub const SOUND_VANILLA: &str = "vanilla";
pub const SOUND_OFF: &str = "off";
// Eigene Dateien aus sounds_dir() -- danach folgt der Dateiname ohne Endung.
pub const CUSTOM_PREFIX: &str = "custom:";

// Die mitgelieferten Klaenge der Mod, als Namen in audio::bank (assets/sounds/totemingpromax/).
// Reihenfolge = Reihenfolge im Stepper, wie SoundLibrary.BUNDLED.
pub const BUNDLED: [(&str, &str); 4] = [
    ("totemingpromax/glass_break_05", "Glas hoch"),
    ("totemingpromax/glass_break_01", "Glas tief"),
    (ORB, "Orb"),
    ("totemingpromax/click", "Klick"),
];

// Der Orb ist Vanillas Erfahrungsklang (random/orb) und liegt deshalb NICHT bei: er kommt zur
// Laufzeit aus der Installation, ueber das Event, das ihn abspielt. Der Name bleibt der alte,
// damit gespeicherte Einstellungen weiter passen.
pub const ORB: &str = "totemingpromax/orb";
pub const ORB_EVENT: &str = "minecraft:entity.experience_orb.pickup";

pub const VOLUME_MAX: u32 = 200;
const VOLUME_STEP: u32 = 10;
// Ueber 2,0 klemmt Vanillas Klangsystem ohnehin ab, unter 0,5 ebenso (SoundSetting).
pub const PITCH_MIN: u32 = 50;
pub const PITCH_MAX: u32 = 200;
const PITCH_STEP: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundSlot {
    OwnMain,
    OwnOff,
    Other,
    Explosion,
    AnchorCharge,
}

impl SoundSlot {
    pub const ALL: [SoundSlot; 5] = [SoundSlot::OwnMain, SoundSlot::OwnOff, SoundSlot::Other, SoundSlot::Explosion, SoundSlot::AnchorCharge];

    pub fn label(self) -> &'static str {
        match self {
            SoundSlot::OwnMain => "Eigener Pop Hotbar",
            SoundSlot::OwnOff => "Eigener Pop Zweithand",
            SoundSlot::Other => "Fremder Pop",
            SoundSlot::Explosion => "Explosion",
            SoundSlot::AnchorCharge => "Anker laden",
        }
    }

    // Die Lautstaerke-Kategorie des Klangs, den der Platz ersetzt: Pops kommen von Spielern,
    // Explosion und Anker von Bloecken.
    pub fn category(self) -> protocol::packets::SoundCategory {
        match self {
            SoundSlot::OwnMain | SoundSlot::OwnOff | SoundSlot::Other => protocol::packets::SoundCategory::Players,
            SoundSlot::Explosion | SoundSlot::AnchorCharge => protocol::packets::SoundCategory::Blocks,
        }
    }

    fn key(self) -> &'static str {
        match self {
            SoundSlot::OwnMain => "own_main",
            SoundSlot::OwnOff => "own_off",
            SoundSlot::Other => "other",
            SoundSlot::Explosion => "explosion",
            SoundSlot::AnchorCharge => "anchor_charge",
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    // Das Ereignis, das Vanilla an dieser Stelle spielt -- fuer "Vanilla" und zum Erkennen
    // der auszutauschenden Pakete.
    pub fn vanilla_event(self) -> &'static str {
        match self {
            SoundSlot::OwnMain | SoundSlot::OwnOff | SoundSlot::Other => "minecraft:item.totem.use",
            SoundSlot::Explosion => "minecraft:entity.generic.explode",
            SoundSlot::AnchorCharge => "minecraft:block.respawn_anchor.charge",
        }
    }

    // Welcher Platz fuer ein Ton-Paket vom Server zustaendig ist (SoundReplacements.forId).
    // Die Pops kommen nicht als Paket, sondern als Entity-Status -- die stehen hier nicht.
    pub fn replacing(event: &str) -> Option<SoundSlot> {
        let event = event.strip_prefix("minecraft:").unwrap_or(event);
        match event {
            "entity.generic.explode" => Some(SoundSlot::Explosion),
            "block.respawn_anchor.charge" => Some(SoundSlot::AnchorCharge),
            _ => None,
        }
    }
}

// Ein Klangplatz: welcher Klang, wie laut, wie hoch. Lautstaerke und Tonhoehe in Prozent,
// damit settings.conf keine Kommazahlen braucht und der Stepper ganze Schritte macht.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundSetting {
    pub sound: String,
    pub volume: u32,
    pub pitch: u32,
}

impl SoundSetting {
    fn new(sound: &str, volume: u32, pitch: u32) -> Self {
        SoundSetting { sound: sound.to_string(), volume, pitch }
    }

    pub fn is_vanilla(&self) -> bool {
        self.sound == SOUND_VANILLA
    }

    pub fn gain(&self) -> f32 {
        self.volume as f32 / 100.0
    }

    pub fn pitch_factor(&self) -> f32 {
        self.pitch as f32 / 100.0
    }

    fn to_line(&self) -> String {
        format!("{}|{}|{}", self.sound, self.volume, self.pitch)
    }

    // Kaputte Teile lassen den jeweiligen Standardwert stehen (SoundSetting.repair).
    fn read_line(&mut self, value: &str) {
        let mut teile = value.split('|');
        if let Some(sound) = teile.next().map(str::trim).filter(|s| !s.is_empty()) {
            self.sound = sound.to_string();
        }
        if let Some(v) = teile.next().and_then(|v| v.trim().parse::<u32>().ok()) {
            self.volume = v.min(VOLUME_MAX);
        }
        if let Some(v) = teile.next().and_then(|v| v.trim().parse::<u32>().ok()) {
            self.pitch = v.clamp(PITCH_MIN, PITCH_MAX);
        }
    }
}

// Anzeigename eines Klangwerts (SoundLibrary.displayName).
pub fn sound_label(sound: &str) -> String {
    match sound {
        SOUND_VANILLA => "Vanilla".to_string(),
        SOUND_OFF => "Aus".to_string(),
        _ => {
            if let Some(name) = sound.strip_prefix(CUSTOM_PREFIX) {
                return name.to_string();
            }
            BUNDLED.iter().find(|(id, _)| *id == sound).map(|(_, label)| label.to_string()).unwrap_or_else(|| sound.to_string())
        }
    }
}

// Alle waehlbaren Werte in Stepper-Reihenfolge: Sonderwerte, mitgelieferte, eigene.
pub fn sound_choices(custom: &[String]) -> Vec<String> {
    let mut all = vec![SOUND_VANILLA.to_string(), SOUND_OFF.to_string()];
    all.extend(BUNDLED.iter().map(|(id, _)| id.to_string()));
    all.extend(custom.iter().map(|name| format!("{CUSTOM_PREFIX}{name}")));
    all
}

// Naechster Wert, oder der erste, falls der aktuelle nicht mehr existiert (etwa weil die
// eigene Datei geloescht wurde) -- SoundLibrary.next.
pub fn next_sound(current: &str, dir: i8, custom: &[String]) -> String {
    let choices = sound_choices(custom);
    match choices.iter().position(|c| c == current) {
        Some(i) => choices[(i as i32 + dir as i32).rem_euclid(choices.len() as i32) as usize].clone(),
        None => choices[0].clone(),
    }
}

// --- Einstellungen --------------------------------------------------------------------

pub const MIN_OFFSET: u32 = 6;
pub const MAX_OFFSET: u32 = 40;
pub const MIN_SIZE: u32 = 1;
pub const MAX_SIZE: u32 = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct TotemingSettings {
    // Totem-Warnung (Fadenkreuz-X).
    pub warning: bool,
    pub offhand_color: IndicatorColor,
    pub hotbar_color: IndicatorColor,
    // Abstand der X-Mitte vom Fadenkreuz und halbe Armlaenge, beides in GUI-Pixeln.
    pub offset: u32,
    pub size: u32,
    pub shadow: bool,
    // Totem-Vorschau (Slot-Overlay).
    pub slots: bool,
    pub backdrop: Backdrop,
    // true: fehlende + 1 zeigen, damit der Ersatz fuer den naechsten Pop gleich mit dabei ist.
    pub extra: bool,
    // Pop-Klaenge.
    pub sounds: bool,
    pub sound: [SoundSetting; 5],
    // Pop-Optik.
    pub visuals: bool,
    pub hide_own_floating: bool,
    pub hide_own_particles: bool,
    // Standardmaessig an gelassen: die Wolke ist der einzige Hinweis, WER gepoppt hat.
    pub hide_other_particles: bool,
}

impl Default for TotemingSettings {
    // Die Standardwerte der Mod -- sie geben das wieder, was Totem Pop Plus und das
    // Resourcepack "Click Explosion" vorher zusammen ergeben haben.
    fn default() -> Self {
        TotemingSettings {
            warning: true,
            offhand_color: IndicatorColor::Red,
            hotbar_color: IndicatorColor::LightBlue,
            offset: 13,
            size: 3,
            shadow: true,
            slots: true,
            backdrop: Backdrop::Shadow,
            extra: true,
            sounds: true,
            sound: [
                SoundSetting::new("totemingpromax/glass_break_05", 100, 200),
                SoundSetting::new("totemingpromax/glass_break_01", 100, 130),
                SoundSetting::new("totemingpromax/orb", 100, 100),
                SoundSetting::new("totemingpromax/click", 100, 100),
                SoundSetting::new(SOUND_OFF, 100, 100),
            ],
            visuals: true,
            hide_own_floating: false,
            hide_own_particles: false,
            hide_other_particles: false,
        }
    }
}

// Was ein Klick im Modul-Menue veraendert. Die Richtung (-1/+1) steckt im Wert, damit ein
// Stepper nicht zwei Varianten braucht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleWarning,
    OffhandColor(i8),
    HotbarColor(i8),
    Offset(i8),
    Size(i8),
    ToggleShadow,
    ToggleSlots,
    Backdrop(i8),
    ToggleExtra,
    ToggleSounds,
    Sound(SoundSlot, i8),
    Volume(SoundSlot, i8),
    Pitch(SoundSlot, i8),
    ToggleVisuals,
    ToggleHideOwnFloating,
    ToggleHideOwnParticles,
    ToggleHideOtherParticles,
}

impl Action {
    // Der Klangplatz, den diese Aktion veraendert -- danach wird er zum Vorhoeren gespielt.
    pub fn sound_slot(self) -> Option<SoundSlot> {
        match self {
            Action::Sound(slot, _) | Action::Volume(slot, _) | Action::Pitch(slot, _) => Some(slot),
            _ => None,
        }
    }
}

fn step_u32(value: u32, dir: i8, step: u32, min: u32, max: u32) -> u32 {
    if dir < 0 {
        value.saturating_sub(step).max(min)
    } else {
        (value + step).min(max)
    }
}

impl TotemingSettings {
    pub fn slot(&self, slot: SoundSlot) -> &SoundSetting {
        &self.sound[slot.index()]
    }

    // `custom`: die eigenen Klangdateien (nur fuer Action::Sound gebraucht).
    pub fn apply(&mut self, action: Action, custom: &[String]) {
        match action {
            Action::ToggleWarning => self.warning = !self.warning,
            Action::OffhandColor(d) => self.offhand_color = self.offhand_color.step(d),
            Action::HotbarColor(d) => self.hotbar_color = self.hotbar_color.step(d),
            Action::Offset(d) => self.offset = step_u32(self.offset, d, 1, MIN_OFFSET, MAX_OFFSET),
            Action::Size(d) => self.size = step_u32(self.size, d, 1, MIN_SIZE, MAX_SIZE),
            Action::ToggleShadow => self.shadow = !self.shadow,
            Action::ToggleSlots => self.slots = !self.slots,
            Action::Backdrop(d) => self.backdrop = self.backdrop.step(d),
            Action::ToggleExtra => self.extra = !self.extra,
            Action::ToggleSounds => self.sounds = !self.sounds,
            Action::Sound(slot, d) => {
                let s = &mut self.sound[slot.index()];
                s.sound = next_sound(&s.sound, d, custom);
            }
            Action::Volume(slot, d) => {
                let s = &mut self.sound[slot.index()];
                s.volume = step_u32(s.volume, d, VOLUME_STEP, 0, VOLUME_MAX);
            }
            Action::Pitch(slot, d) => {
                let s = &mut self.sound[slot.index()];
                s.pitch = step_u32(s.pitch, d, PITCH_STEP, PITCH_MIN, PITCH_MAX);
            }
            Action::ToggleVisuals => self.visuals = !self.visuals,
            Action::ToggleHideOwnFloating => self.hide_own_floating = !self.hide_own_floating,
            Action::ToggleHideOwnParticles => self.hide_own_particles = !self.hide_own_particles,
            Action::ToggleHideOtherParticles => self.hide_other_particles = !self.hide_other_particles,
        }
    }

    // Eine Zeile aus settings.conf. false = gehoert nicht hierher.
    pub fn parse_line(&mut self, key: &str, value: &str) -> bool {
        let Some(key) = key.strip_prefix("toteming_") else { return false };
        let flag = value == "true";
        match key {
            "warning" => self.warning = flag,
            "warning_offhand_color" => self.offhand_color = IndicatorColor::from_key(value).unwrap_or(self.offhand_color),
            "warning_hotbar_color" => self.hotbar_color = IndicatorColor::from_key(value).unwrap_or(self.hotbar_color),
            "warning_offset" => {
                if let Ok(v) = value.parse::<u32>() {
                    self.offset = v.clamp(MIN_OFFSET, MAX_OFFSET);
                }
            }
            "warning_size" => {
                if let Ok(v) = value.parse::<u32>() {
                    self.size = v.clamp(MIN_SIZE, MAX_SIZE);
                }
            }
            "warning_shadow" => self.shadow = flag,
            "slots" => self.slots = flag,
            "slots_backdrop" => {
                if let Some(b) = Backdrop::ALL.into_iter().find(|b| b.key() == value) {
                    self.backdrop = b;
                }
            }
            "slots_extra" => self.extra = flag,
            "sounds" => self.sounds = flag,
            "visuals" => self.visuals = flag,
            "hide_own_floating" => self.hide_own_floating = flag,
            "hide_own_particles" => self.hide_own_particles = flag,
            "hide_other_particles" => self.hide_other_particles = flag,
            _ => {
                let Some(slot_key) = key.strip_prefix("sound_") else { return false };
                let Some(slot) = SoundSlot::ALL.into_iter().find(|s| s.key() == slot_key) else { return false };
                self.sound[slot.index()].read_line(value);
            }
        }
        true
    }

    pub fn serialize(&self) -> String {
        let mut out = format!(
            "toteming_warning={}\ntoteming_warning_offhand_color={}\ntoteming_warning_hotbar_color={}\ntoteming_warning_offset={}\ntoteming_warning_size={}\ntoteming_warning_shadow={}\n\
             toteming_slots={}\ntoteming_slots_backdrop={}\ntoteming_slots_extra={}\ntoteming_sounds={}\n\
             toteming_visuals={}\ntoteming_hide_own_floating={}\ntoteming_hide_own_particles={}\ntoteming_hide_other_particles={}\n",
            self.warning,
            self.offhand_color.key(),
            self.hotbar_color.key(),
            self.offset,
            self.size,
            self.shadow,
            self.slots,
            self.backdrop.key(),
            self.extra,
            self.sounds,
            self.visuals,
            self.hide_own_floating,
            self.hide_own_particles,
            self.hide_other_particles,
        );
        for slot in SoundSlot::ALL {
            out.push_str(&format!("toteming_sound_{}={}\n", slot.key(), self.slot(slot).to_line()));
        }
        out
    }
}

// --- Was fehlt ------------------------------------------------------------------------

// Der Inhalt der Haende und des Inventars, soweit er fuer Warnung und Vorschau zaehlt.
#[derive(Clone, Copy, Debug, Default)]
pub struct TotemState {
    pub offhand_totem: bool,
    // Ein Schild in der Zweithand ist eine bewusste Entscheidung -- dann wird nicht gewarnt.
    pub offhand_shield: bool,
    pub hotbar_totem: bool,
    // Irgendwo im Inventar (Hauptinventar, Hotbar, Zweithand).
    pub any_totem: bool,
}

// Wie viele Totems an ihrem Platz fehlen: eins fuer die Zweithand, eins fuer die Hotbar
// (TotemWarningHud.countMissing). 0, wenn gar keins da ist oder die Zweithand ein Schild
// haelt. Warnung UND Vorschau lesen diese eine Zahl, damit beide dieselbe Bedingung sehen.
pub fn count_missing(state: TotemState) -> u32 {
    if state.offhand_shield || !state.any_totem {
        return 0;
    }
    u32::from(!state.offhand_totem) + u32::from(!state.hotbar_totem)
}

// --- Geometrie ------------------------------------------------------------------------

// Das X als zwei Diagonalen (statt der Einzelpixel von TotemWarningHud.drawCross): gleich
// gross wie das Pixel-X der Mod -- (2*arm+1) GUI-Pixel zu je `unit` Bildschirmpixeln --,
// um `center`. `extra` verlaengert beide Enden (fuer die Kontur, die ueber die Enden reicht).
// Die Strichdicke (cross_thickness) ragt an den Enden quer ueber die Diagonale hinaus; die
// Enden ruecken deshalb um die halbe Dicke nach innen, damit das X im alten Kasten bleibt.
pub fn cross_segments(center: (f32, f32), arm: u32, unit: f32, extra: f32) -> [((f32, f32), (f32, f32)); 2] {
    let halb = (arm as f32 + 0.5) * unit - cross_thickness(unit) * std::f32::consts::FRAC_1_SQRT_2 / 2.0;
    let h = halb + extra * std::f32::consts::FRAC_1_SQRT_2;
    let (x, y) = center;
    [((x - h, y - h), (x + h, y + h)), ((x - h, y + h), (x + h, y - h))]
}

// Strichdicke des X in Bildschirmpixeln. Die Pixeltreppe der Mod legt je GUI-Pixel Laenge
// entlang der Diagonale ein Quadrat (unit^2) auf sqrt(2)*unit Strecke -- im Mittel also nur
// unit/sqrt(2) = 0,71 unit dick. Mit 0,8 wirkt die glatte Linie so kraeftig wie das Original
// (die frueheren 1,3 waren zu fett).
pub fn cross_thickness(unit: f32) -> f32 {
    unit * 0.8
}

// Welche Totem-Slots die Vorschau zeigt, naechstgelegene zuerst (TotemSlotOverlayHud).
//
// `totem_slots`: die Slots des Hauptinventars und der Hotbar, in denen ein Totem liegt, an
// ihrer Lage im gleich geoeffneten Inventar. `center`: wo der Zeiger beim Oeffnen steht
// (Bildschirmmitte). Der Slot direkt darunter bleibt frei -- sonst laege die Vorschau beim
// Zielen genau im Weg.
pub fn overlay_slots(totem_slots: &[SlotRect], center: (f32, f32), missing: u32, extra: bool) -> Vec<SlotRect> {
    if missing == 0 {
        return Vec::new();
    }
    let mut kandidaten: Vec<SlotRect> = totem_slots.iter().copied().filter(|s| !s.contains(center.0, center.1)).collect();
    let abstand = |s: &SlotRect| {
        let dx = s.x + s.size / 2.0 - center.0;
        let dy = s.y + s.size / 2.0 - center.1;
        dx * dx + dy * dy
    };
    kandidaten.sort_by(|a, b| abstand(a).total_cmp(&abstand(b)));
    let wanted = if extra { missing + 1 } else { missing } as usize;
    kandidaten.truncate(wanted);
    kandidaten
}

// --- Klaenge fuer den Sofort-Haken ----------------------------------------------------

// Eine fertig aufgeloeste Stimme: Aufnahme, Pegel (inkl. Hauptlautstaerke), Tonhoehe.
#[derive(Clone)]
pub struct PopVoice {
    pub sample: Arc<Wav>,
    pub gain: f32,
    pub pitch: f32,
}

// Was beim EIGENEN Pop im Netzwerk-Task gespielt wird. Der Hauptthread rechnet das vorab aus
// (s. App::publish_own_pop_plan), der Haken greift nur noch zu -- Dateien laden oder
// Einstellungen lesen darf er nicht, er soll den Ton ohne jeden Umweg abspielen.
#[derive(Clone, Default)]
pub enum OwnPopPlan {
    #[default]
    Silent,
    // Der synthetische Signalton der Totem-Anzeige (modules::TotemHand::sound_hz).
    Tone,
    Samples {
        main: Option<PopVoice>,
        off: Option<PopVoice>,
    },
}

// --- Eigene Klangdateien --------------------------------------------------------------

// Die .ogg/.wav-Dateien des Nutzers. Neu eingelesen bei jedem Weiterschalten im Menue, damit
// eine gerade abgelegte Datei ohne Neustart auftaucht (wie SoundLibrary.choices).
#[derive(Default)]
pub struct CustomSounds {
    geladen: HashMap<String, Option<Arc<Wav>>>,
}

impl CustomSounds {
    pub fn dir() -> Option<std::path::PathBuf> {
        crate::shared_config_dir().map(|d| d.join("sounds"))
    }

    // Namen ohne Endung, sortiert -- damit der Stepper zwischen zwei Starts nicht springt.
    pub fn scan() -> Vec<String> {
        let Some(dir) = Self::dir() else { return Vec::new() };
        let Ok(eintraege) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut namen: Vec<String> = eintraege
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ogg") || e.eq_ignore_ascii_case("wav")))
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        namen.sort();
        namen.dedup();
        namen
    }

    // Einmal dekodiert, danach aus dem Zwischenspeicher. Nicht lesbar = still (und nur
    // einmal ins Log geschrieben).
    pub fn get(&mut self, name: &str) -> Option<Arc<Wav>> {
        if let Some(vorhanden) = self.geladen.get(name) {
            return vorhanden.clone();
        }
        let geladen = Self::dir().and_then(|dir| {
            ["ogg", "wav", "OGG", "WAV"].iter().find_map(|ext| {
                let pfad = dir.join(format!("{name}.{ext}"));
                let bytes = std::fs::read(&pfad).ok()?;
                let wav = if ext.eq_ignore_ascii_case("ogg") {
                    audio::vorbis::decode(&bytes).map_err(|e| format!("{e:?}"))
                } else {
                    Wav::from_bytes(&bytes).map_err(|e| format!("{e:?}"))
                };
                match wav {
                    Ok(w) if !w.samples.is_empty() => Some(Arc::new(w)),
                    Ok(_) => None,
                    Err(e) => {
                        eprintln!("[audio] eigener Klang {} nicht lesbar: {e}", pfad.display());
                        None
                    }
                }
            })
        });
        self.geladen.insert(name.to_string(), geladen.clone());
        geladen
    }

    // Nach dem Weiterschalten: eine inzwischen ersetzte Datei soll neu gelesen werden.
    pub fn forget(&mut self) {
        self.geladen.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standardwerte_wie_in_der_mod() {
        let s = TotemingSettings::default();
        assert_eq!(s.slot(SoundSlot::OwnMain), &SoundSetting::new("totemingpromax/glass_break_05", 100, 200));
        assert_eq!(s.slot(SoundSlot::OwnOff).pitch, 130);
        assert_eq!(s.slot(SoundSlot::Other).sound, "totemingpromax/orb");
        assert_eq!(s.slot(SoundSlot::Explosion).sound, "totemingpromax/click");
        assert_eq!(s.slot(SoundSlot::AnchorCharge).sound, SOUND_OFF);
        assert_eq!((s.offhand_color, s.hotbar_color, s.offset, s.size), (IndicatorColor::Red, IndicatorColor::LightBlue, 13, 3));
    }

    #[test]
    fn speichern_und_laden_ergibt_dasselbe() {
        let mut s = TotemingSettings {
            warning: false,
            hotbar_color: IndicatorColor::Pink,
            offset: 20,
            backdrop: Backdrop::Box,
            hide_other_particles: true,
            ..Default::default()
        };
        s.sound[SoundSlot::Other.index()] = SoundSetting::new("custom:mein_pop", 150, 75);
        let mut geladen = TotemingSettings::default();
        for line in s.serialize().lines() {
            let (k, v) = line.split_once('=').unwrap();
            assert!(geladen.parse_line(k, v), "Zeile nicht erkannt: {line}");
        }
        assert_eq!(geladen, s);
    }

    #[test]
    fn kaputte_werte_werden_geklemmt() {
        let mut s = TotemingSettings::default();
        s.parse_line("toteming_warning_offset", "999");
        s.parse_line("toteming_sound_own_main", "x|5000|1");
        assert_eq!(s.offset, MAX_OFFSET);
        assert_eq!(s.slot(SoundSlot::OwnMain), &SoundSetting::new("x", VOLUME_MAX, PITCH_MIN));
        assert!(!s.parse_line("fov", "90"), "fremde Schlüssel gehören nicht hierher");
    }

    #[test]
    fn klangauswahl_laeuft_um_und_faengt_verschwundene_dateien() {
        let custom = vec!["eigen".to_string()];
        assert_eq!(next_sound(SOUND_VANILLA, -1, &custom), "custom:eigen");
        assert_eq!(next_sound("custom:eigen", 1, &custom), SOUND_VANILLA);
        assert_eq!(next_sound(SOUND_OFF, 1, &custom), "totemingpromax/glass_break_05");
        assert_eq!(next_sound("custom:geloescht", 1, &custom), SOUND_VANILLA);
        assert_eq!(sound_label("totemingpromax/orb"), "Orb");
        assert_eq!(sound_label("custom:eigen"), "eigen");
    }

    #[test]
    fn fehlende_totems_wie_in_der_mod() {
        let voll = TotemState { offhand_totem: true, offhand_shield: false, hotbar_totem: true, any_totem: true };
        assert_eq!(count_missing(voll), 0);
        assert_eq!(count_missing(TotemState { offhand_totem: false, ..voll }), 1);
        assert_eq!(count_missing(TotemState { offhand_totem: false, hotbar_totem: false, ..voll }), 2);
        assert_eq!(count_missing(TotemState { offhand_totem: false, hotbar_totem: false, any_totem: false, ..voll }), 0, "ohne Totem keine Warnung");
        assert_eq!(count_missing(TotemState { offhand_totem: false, offhand_shield: true, ..voll }), 0, "Schild ist Absicht");
    }

    #[test]
    fn x_ist_symmetrisch_um_die_mitte() {
        let [a, b] = cross_segments((100.0, 50.0), 3, 2.0, 0.0);
        for (p0, p1) in [a, b] {
            assert!(((p0.0 + p1.0) / 2.0 - 100.0).abs() < 1e-4);
            assert!(((p0.1 + p1.1) / 2.0 - 50.0).abs() < 1e-4);
        }
        // Mit halber Strichdicke passt es in den Kasten des Pixel-X: (2*3+1) GUI-Pixel zu je 2.
        let rand = a.1 .0 + cross_thickness(2.0) * std::f32::consts::FRAC_1_SQRT_2 / 2.0;
        assert!((rand - 107.0).abs() < 1e-3, "{rand}");
        let [lang, _] = cross_segments((100.0, 50.0), 3, 2.0, 1.0);
        assert!(lang.1 .0 > a.1 .0, "Kontur reicht über die Enden");
    }

    fn slot(index: usize, x: f32, y: f32) -> SlotRect {
        SlotRect { index, x, y, size: 16.0, pad: 1.0 }
    }

    #[test]
    fn vorschau_nimmt_die_naechsten_und_laesst_die_mitte_frei() {
        let center = (100.0, 100.0);
        let slots = [slot(1, 92.0, 92.0), slot(2, 130.0, 92.0), slot(3, 200.0, 92.0), slot(4, 60.0, 92.0)];
        let gezeigt: Vec<usize> = overlay_slots(&slots, center, 1, true).iter().map(|s| s.index).collect();
        assert_eq!(gezeigt, vec![4, 2], "Slot 1 liegt unter dem Zeiger, dann die zwei nächsten (4 ist 32 px weg, 2 ist 38)");
        assert_eq!(overlay_slots(&slots, center, 1, false).len(), 1);
        assert!(overlay_slots(&slots, center, 0, true).is_empty());
    }
}
