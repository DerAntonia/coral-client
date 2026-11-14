// Frame-Zeiten und -Statistik, Zoom und Empfindlichkeit, Zahlen- und Wertformatierung.

use crate::*;

// Aufschluesselung eines Frames in Wartezeit (get_current_texture(), bei Fifo bis zum
// VBlank) und tatsaechliche Arbeit (Vertex-Aufbau, Encoding, Submit). Ohne diese Trennung
// waere die Wartezeit als "Arbeit" ausgewiesen -- irrefuehrend gerade bei Fifo, wo sie den
// groessten Teil der gemessenen Zeit ausmacht.
pub(crate) struct FrameTiming {
    // Gewartet statt gearbeitet: auf das vorige Frame der GPU (Zero-Modus), aufs Swapchain-Bild,
    // auf den Termin der Riss-Steuerung, auf das Present (s. FrameStats).
    pub(crate) idle_ms: f32,
    // Aufschluesselung fuer den Frame-Profiler: Aufbau/Encoding bis vor dem Submit, das
    // Submit selbst und das Present (s. GpuState::submit_and_present).
    pub(crate) encode_ms: f32,
    pub(crate) submit_ms: f32,
    pub(crate) present_ms: f32,
    // Zeitpunkte (CLOCK_MONOTONIC, ns, s. vblank::monotonic_ns) fuer die Phasenmessung des
    // Profilers: Present-Aufruf und -- nur mit GpuState::measure_gpu_done -- GPU fertig.
    pub(crate) present_ns: i64,
    pub(crate) gpu_done_ns: Option<i64>,
    // Nur mit Riss-Steuerung: wann Frame und GPU fertig waren, bevor auf den Termin gewartet
    // wurde (fuer den Vorlauf, s. App::tear_lead_ns).
    pub(crate) ready_ns: Option<i64>,
}

// Die TAGFARBE des Himmels im ANZEIGE-Raum (sRGB). Frueher stand hier die fertige lineare
// Clear-Farbe; seit der Himmel der Tageszeit folgt (s. world::sky), wird im Anzeige-Raum
// gerechnet und erst am Ende umgerechnet -- ein Faktor auf lineare Werte wuerde die
// Daemmerung falsch abdunkeln, weil die Helligkeitskurve fuer das Auge gilt, nicht fuer die
// Leistung.
pub(crate) const SKY_DAY_SRGB: [f32; 3] = [0.45, 0.65, 0.95];

// Gleichmaessige Helligkeit fuer Entity-Modelle. Dieser Client fuehrt (noch) kein
// Entity-Licht -- Vanilla nimmt den Lichtwert am Entity-Block plus die Zwei-Licht-Diffusion
// der Flaechennormalen. Ein fester, leicht abgedunkelter Wert sieht neben den (per Richtung
// schattierten) Bloecken plausibel aus, ohne Licht zu erfinden, das wir nicht kennen.
// Grundfarbe der Entity-Modelle, BEVOR das Richtungslicht draufkommt (s.
// world::diffuse_shade). Vanilla startet dort bei Weiss und multipliziert mit dem Licht, die
// Flaechen liegen damit zwischen 0.4 und 1.0.
//
// Vorher stand hier 0.9 als flache Ersatzhelligkeit fuer "wir haben kein Licht" -- mit echter
// Schattierung wuerde das alles um 10 % zu dunkel machen.
//
// Eine Lichtkarte gibt es hier bewusst NICHT. Vanilla multipliziert zusaetzlich mit dem
// Block-/Himmelslicht am Standort des Entities; dieser Client wertet Lichtdaten ueberhaupt
// nicht aus -- weder fuer Bloecke noch fuer Entities -- und rendert damit durchgaengig
// fullbright. Das ist eine Entscheidung, keine Luecke: fuer PvP ist
// dauerhaftes Fullbright gewollt, und Entities passen so zum Rest der Welt statt in einer
// dunklen Hoehle als einzige Objekte abzudunkeln.
// Vanillas Schwungdauer fuer Spieler (LivingEntity.getHandSwingDuration ueber die
// Swing-Animation des Items -- 6 Ticks fuer die Standardwaffe).
// Entscheidet fuer EINEN Tick, wie viele Benutzungen ausgeloest werden und wie der Cooldown
// danach steht. Als freie Funktion, damit die Regel ohne laufendes Fenster pruefbar ist --
// sie ist der ganze Unterschied zwischen "Klicks gehen verloren" und nicht.
//
// Rueckgabe: (Anzahl Benutzungen in diesem Tick, neuer Cooldown).
//
// Das bildet Vanillas handleKeybinds 1:1 ab:
//
//     while (useKey.wasPressed()) doItemUse();                       // ALLE gemerkten Klicks
//     if (useKey.isPressed() && itemUseCooldown == 0 && !isUsingItem()) doItemUse();
//
// ALLE gemerkten Klicks feuern also im SELBEN Tick, nicht einer pro Tick. Frueher stand hier
// genau das (einer pro Tick), und das kostete den Anchor-Airplace: der braucht zwei Klicks in
// einem Tick, damit beide dieselbe Zielposition tragen (s. tick_item_use).
//
// Reihenfolge wie in Vanilla: erst laeuft der Cooldown herunter, dann wird entschieden.
//
// `repeat` ist der Schalter aus den Einstellungen (s. Settings::hold_use_repeat). Er wirkt NUR
// auf das Halten, nie auf gemerkte Klicks: wer viermal klickt, will auch viermal setzen --
// abgeschaltet wird das WIEDERHOLEN, nicht das Puffern.
pub(crate) const fn item_uses_this_tick(queue: u32, cooldown: u32, held: bool, repeat: bool) -> (u32, u32) {
    let cooldown = cooldown.saturating_sub(1);
    if queue > 0 {
        // Gemerkte Klicks zaehlen IMMER, auch waehrend des Cooldowns -- Vanillas doItemUse
        // prueft ihn ebenfalls nicht, es setzt ihn nur.
        return (queue, ITEM_USE_COOLDOWN_TICKS);
    }
    if held && repeat && cooldown == 0 {
        return (1, ITEM_USE_COOLDOWN_TICKS);
    }
    (0, cooldown)
}

// Items, deren `use` schon im Client ein Stueck verbraucht (s. App::consume_hand_item).
pub(crate) const THROWN_ITEMS: [&str; 9] =
    ["splash_potion", "lingering_potion", "snowball", "egg", "blue_egg", "brown_egg", "ender_pearl", "experience_bottle", "wind_charge"];

// MinecraftClient.itemUseCooldown: 4 Ticks zwischen zwei Benutzungen bei GEHALTENER Taste.
pub(crate) const ITEM_USE_COOLDOWN_TICKS: u32 = 4;
// Grundwerte der Attribute block_interaction_range (4.5) und entity_interaction_range (3.0),
// bis der Server andere schickt (s. App::attr_block_range). Zwei verschiedene Zahlen, das ist
// kein Versehen.
pub(crate) const BLOCK_INTERACTION_REACH: f64 = 4.5;
pub(crate) const ENTITY_INTERACTION_REACH: f64 = 3.0;
pub(crate) const PLAYER_SWING_TICKS: u32 = 6;

// Schwungfortschritt aus dem rueckwaerts laufenden Zaehler (s. App::own_swing_ticks).
// PLAYER_SWING_TICKS + 1 heisst "angestossen, noch nicht getickt" (Vanillas handSwingTicks =
// -1): noch kein Ausschlag. Wird der Schwung im Tick VOR dem Herunterzaehlen gesetzt, steht der
// Zaehler danach auf PLAYER_SWING_TICKS und der erste Frame beginnt bei 0 -- ohne diese
// Stufe sprang das Item beim Schlag gleich ein bis zwei Sechstel weit.
// Deckel bei 1: dort steht das Item wieder in Ruhelage (alle Sinusanteile sind 0); dahinter
// kippen die Formeln und das Item stand fuer einige Frames weit hinten.
// `dauer`: Schwungdauer des Haupthand-Items (Speere laenger, s. App::own_swing_duration).
pub(crate) fn swing_progress_from_ticks(ticks: u32, tick_progress: f32, dauer: u32) -> f32 {
    if ticks == 0 || ticks > dauer {
        return 0.0;
    }
    ((dauer - ticks) as f32 + tick_progress).min(dauer as f32) / dauer as f32
}

// Neuer Schwung -- aber wie Vanillas LivingEntity.swingHand nur, wenn gerade keiner laeuft,
// der laufende schon mindestens die halbe Dauer hinter sich hat oder eben erst angestossen
// ist (handSwingTicks < 0). Beim Abbauen schwingt der Arm JEDEN Tick; ohne diese Regel fing
// der Schwung jeden Tick von vorn an und die Spitzhacke sprang staendig zurueck.
pub(crate) const fn restart_swing(ticks: u32, dauer: u32) -> u32 {
    let vergangen = dauer as i32 - ticks as i32;
    if ticks == 0 || vergangen >= dauer as i32 / 2 || vergangen < 0 {
        dauer + 1
    } else {
        ticks
    }
}

pub(crate) const ENTITY_BRIGHTNESS: f32 = 1.0;

// Fallback-Cap, falls die Refresh-Rate des Monitors nicht ermittelt werden kann.
pub(crate) const FALLBACK_FRAME_INTERVAL: Duration = Duration::from_millis(16);

// Radiant pro Pixel Maus-Delta -- willkuerlich gewaehlt, bei Bedarf anpassen.
// Consumable-Optimizer, Durchdruecken: Obergrenze und Schritt in Millisekunden (ein Tick).
pub(crate) const EAT_HOLD_THROUGH_MAX: u32 = 500;
pub(crate) const EAT_HOLD_THROUGH_STEP: u32 = 50;
// Zoom-Modul (s. ModuleId::Zoom): Staerke in Zehnteln (Sichtfeld geteilt durch), Faktor auf
// die Maus-Empfindlichkeit in Prozent, und wie schnell der Zoom ein- und ausfaehrt
// (Zeitkonstante der Annaeherung).
// Modul coral-clip: Dauer des Clips in Sekunden.
pub(crate) const CLIP_SECONDS_MIN: u32 = 10;
pub(crate) const CLIP_SECONDS_MAX: u32 = 300;
pub(crate) const CLIP_SECONDS_STEP: u32 = 10;
pub(crate) const CLIP_SECONDS_DEFAULT: u32 = 30;
// Haltepunkte je Clip-Dauer (s. Settings::clip_checkpoints).
pub(crate) const CLIP_CHECKPOINTS_MIN: u32 = 1;
pub(crate) const CLIP_CHECKPOINTS_MAX: u32 = 10;
pub(crate) const CLIP_CHECKPOINTS_DEFAULT: u32 = 4;
pub(crate) const ZOOM_FACTOR_MIN: u32 = 15;
pub(crate) const ZOOM_FACTOR_MAX: u32 = 150;
pub(crate) const ZOOM_FACTOR_STEP: u32 = 5;
// Modul "Farbe": Saettigung in Prozent, in Zehnerschritten bis zur doppelten.
pub(crate) const COLOR_SATURATION_MAX: u32 = 200;
pub(crate) const COLOR_SATURATION_STEP: u32 = 10;
pub(crate) const ZOOM_SENS_MIN: u32 = 5;
pub(crate) const ZOOM_SENS_MAX: u32 = 200;
pub(crate) const ZOOM_SENS_STEP: u32 = 5;
pub(crate) const ZOOM_SMOOTH_SECS: f32 = 0.04;

// Sichtfeld und Maus-Faktor bei Zoom-Anteil `amount` (0 = aus, 1 = voll). Das Sichtfeld
// schrumpft geometrisch (fov / Staerke^amount), damit das Einfahren gleichmaessig wirkt
// statt am Ende zu springen.
pub(crate) fn zoomed_fov(fov: f32, factor_tenths: u32, amount: f32) -> f32 {
    fov / (factor_tenths as f32 / 10.0).powf(amount)
}

pub(crate) fn zoom_sens_factor(sens_percent: u32, amount: f32) -> f32 {
    1.0 + (sens_percent as f32 / 100.0 - 1.0) * amount
}

// Sens-Rechner (s. MenuScreen::SettingsSensCalc): Zeilen und DPI-Grenzen.
pub(crate) const SENS_CALC_MC_ROW: usize = 0;
pub(crate) const SENS_CALC_RESULT_ROW: usize = 1;
pub(crate) const SENS_CALC_DPI_ROW: usize = 3;
pub(crate) const SENS_CALC_CM_MC_ROW: usize = 4;
pub(crate) const SENS_CALC_CM_OWN_ROW: usize = 5;
pub(crate) const SENS_CALC_DPI_MIN: u32 = 50;
pub(crate) const SENS_CALC_DPI_MAX: u32 = 64_000;

// Minecraft-Empfindlichkeit (Optionen-Regler in Prozent, 0..200, 100 % = 0,5) in unsere
// Einheit (Radiant je Maus-Zaehler). Vanilla (Mouse.updateMouse):
//     d = sens * 0.6 + 0.2;  e = d^3 * 8;  yaw += dx * e * 0.15  (Grad)
pub(crate) fn minecraft_sens_to_client(percent: f32) -> f32 {
    let d = (percent / 200.0) * 0.6 + 0.2;
    (d * d * d * 8.0 * 0.15).to_radians()
}

// Mauswege fuer eine volle Drehung, in cm: 360 Grad geteilt durch Grad je Zaehler ergibt die
// Zaehler, geteilt durch DPI die Zoll.
pub(crate) fn cm_per_360(sens_rad: f32, dpi: u32) -> f32 {
    360.0 / sens_rad.to_degrees() / dpi as f32 * 2.54
}

pub(crate) const DEFAULT_MOUSE_SENSITIVITY: f32 = 0.0025;
pub(crate) const SENSITIVITY_MIN: f32 = 0.0005;
pub(crate) const SENSITIVITY_MAX: f32 = 0.0100;
pub(crate) const SENSITIVITY_STEP: f32 = 0.0005;

// Empfindlichkeit des MAUSZEIGERS in den Menues/Overlays -- ein reiner Faktor auf das rohe
// Pixel-Delta (1.0 = der Zeiger legt genau so viele Bildschirmpixel zurueck wie die Maus
// meldet). Bewusst eine eigene Einstellung und nicht dieselbe wie fuer die Kamera: die eine
// ist Radiant pro Pixel und haengt am Sichtfeld, die andere ist ein Pixel-zu-Pixel-Verhaeltnis
// und haengt an der Bildschirmgroesse. Ein gemeinsamer Regler zwingt einen, den einen Wert
// schlecht einzustellen, damit der andere passt.
pub(crate) const DEFAULT_CURSOR_SENSITIVITY: f32 = 1.0;
pub(crate) const CURSOR_SENSITIVITY_MIN: f32 = 0.05;
pub(crate) const CURSOR_SENSITIVITY_MAX: f32 = 5.0;
pub(crate) const CURSOR_SENSITIVITY_STEP: f32 = 0.05;

// Anzeigewert der Kamera-Empfindlichkeit (das Tausendfache des internen Radiant/Pixel-Werts).
// VIER Nachkommastellen: der Stepper springt in 0.5er-Schritten, wer feiner zielen will, tippt
// die Zahl direkt ein -- und dabei soll auch die vierte Stelle stehen bleiben statt gerundet
// zu werden. Mit nur zweien wurde eine eingegebene 1.2345 als "1.23" angezeigt: der Wert war
// korrekt uebernommen, sah aber aus, als haette das Feld die Eingabe verworfen (genau dieselbe
// Verwirrung gab es vorher schon mit einer einzigen Stelle). Nachlaufende Nullen fallen weg,
// damit die Stepper-Werte weiter als "2.5" und nicht als "2.5000" dastehen.
pub(crate) fn format_sensitivity(sensitivity: f32) -> String {
    trimme_nullen(format!("{:.4}", sensitivity * 1000.0))
}

// Anzeigewert der Zeiger-Empfindlichkeit -- derselbe Zuschnitt, aber ohne den 1000er-Faktor:
// der Wert IST schon der Faktor, den man einstellt.
// Ein Stepper-Schritt der jeweiligen Empfindlichkeit, inklusive Klemmen. Als eigene
// Funktionen, damit Schrittweite und Grenzen pruefbar sind, ohne eine ganze App aufzubauen --
// und damit die beiden Regler nachweislich getrennt bleiben.
pub(crate) fn step_sensitivity(aktuell: f32, hoch: bool) -> f32 {
    let schritt = if hoch { SENSITIVITY_STEP } else { -SENSITIVITY_STEP };
    (aktuell + schritt).clamp(SENSITIVITY_MIN, SENSITIVITY_MAX)
}

pub(crate) fn step_cursor_sensitivity(aktuell: f32, hoch: bool) -> f32 {
    let schritt = if hoch { CURSOR_SENSITIVITY_STEP } else { -CURSOR_SENSITIVITY_STEP };
    (aktuell + schritt).clamp(CURSOR_SENSITIVITY_MIN, CURSOR_SENSITIVITY_MAX)
}

pub(crate) fn format_cursor_sensitivity(faktor: f32) -> String {
    trimme_nullen(format!("{faktor:.4}"))
}

pub(crate) fn trimme_nullen(text: String) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

// Wie lange eine Crit-Meldung auf ihren Treffer wartet -- dieselbe Spanne wie fuer fremde
// Entities (s. world::Entity::on_crit), damit sich der eigene Blitz nicht anders verhaelt als
// der der anderen.
// EntityStatuses.ADD_DEATH_PARTICLES: LivingEntity.handleStatus ruft addDeathParticles.
pub(crate) const ENTITY_STATUS_DEATH_PARTICLES: i8 = 60;
// LivingEntity.handleEntityEvent, Fall 2: ein Speer-Ansturm hat etwas getroffen (onKineticHit).
pub(crate) const ENTITY_STATUS_KINETIC_HIT: i8 = 2;
// Session-Info-Tafel in HUD-Einheiten: Innenabstand, Mindestbreite, Luft zwischen den Zeilen.
pub(crate) const SESSION_PAD: f32 = 4.0;
pub(crate) const SESSION_MIN_W: f32 = 90.0;
pub(crate) const SESSION_LINE_GAP: f32 = 2.0;
// LivingEntity.die / ServerPlayer.die: das Entity ist gestorben (an alle, die es sehen).
pub(crate) const ENTITY_STATUS_DIED: i8 = 3;
// Hoechstens so lange bleibt der Pack-Himmel nach einem Dimensionswechsel weg, bis er geladen ist.
pub(crate) const SKY_WORLD_WAIT: Duration = Duration::from_secs(3);
// Schneeball oder Ei schlaegt auf (Snowball/ThrownEgg.handleEntityEvent, Fall 3).
pub(crate) const ENTITY_STATUS_THROWN_HIT: i8 = 3;
// Das eigene Essen/Trinken ist beim Server fertig (LivingEntity.handleEntityEvent, Fall 9).
pub(crate) const ENTITY_STATUS_USE_ITEM_COMPLETE: i8 = 9;
// Hoechstens so oft klingt der Treffer je Entity (onKineticHit: 10 Ticks).
pub(crate) const KINETIC_HIT_FEEDBACK: Duration = Duration::from_millis(500);

pub(crate) const CRIT_GRACE_TICKS: u8 = 2;

// Master-Tonhoehe fuer UI-Klaenge aus Aufnahmen (s. Settings::ui_pitch_semitones). Bewusst
// OHNE Ober-/Untergrenze -- audio::CueSink::play_sample klemmt den Faktor selbst auf ein
// numerisches Minimum (1e-3, gegen Division durch null), eine kreative Obergrenze darueber
// waere nur eine erfundene Einschraenkung ohne technischen Grund.
pub(crate) const UI_PITCH_TOOLTIP: &str =
    "Verschiebt die Tonhöhe der UI-Klänge, die aus Aufnahmen abgespielt werden (s. UiSound::sample). In Halbtönen, nach oben und unten offen; Tonhöhe und Länge hängen dabei zusammen, wie bei einer schneller/langsamer abgespielten Aufnahme -- genau wie in scripts/klaenge-anhoeren.sh. Wirkt NICHT auf Signaltöne wie den Totem-Pop.";

// "+3", "-5", "0" -- ein Vorzeichen nur bei einer Abweichung von null, damit nicht jeder
// Wert ein "+0" oder redundantes "-0" zeigt.
pub(crate) fn format_ui_pitch(halbtoene: i32) -> String {
    match halbtoene.cmp(&0) {
        std::cmp::Ordering::Greater => format!("+{halbtoene}"),
        _ => halbtoene.to_string(),
    }
}

// "256 Frames · 5,3 ms", dahinter in Klammern die gemessene Callback-Groesse, wenn sie von
// der Anfrage abweicht -- das Backend darf die Anfrage umbiegen (Windows tut es meist).
// Ohne ms-Angabe in der Klammer: die las sich wie die Rechenzeit des Callbacks, gemessen wird
// aber nur, wie viele Frames er zuletzt gefuellt hat.
// Zeile "PipeWire-Grenze": ein Knopf mit Beschriftung und Aktion, oder nur ein Text, warum es
// nichts zu tun gibt (s. App::pipewire_knopf).
pub(crate) enum PipewireKnopf {
    Knopf(String, Option<u32>),
    Text(String),
}

// Ab dieser Groesse greift die Vorgabe von pipewire-pulse (128/48000) nicht mehr ein.
pub(crate) const PIPEWIRE_PULSE_MIN: u32 = 128;

pub(crate) fn format_audio_buffer(info: Option<&audio::BufferInfo>) -> String {
    let Some(info) = info else { return "kein Stream".to_string() };
    let ms = |frames: u32| format!("{:.1}", info.frames_to_ms(frames)).replace('.', ",");
    let gemessen = info.callback_frames();
    let mut text = match info.requested_frames {
        Some(n) => format!("{n} Frames · {} ms", ms(n)),
        None => "Backend-Vorgabe".to_string(),
    };
    if gemessen != 0 && Some(gemessen) != info.requested_frames {
        text.push_str(&format!(" (Callback füllte {gemessen} Frames)"));
    }
    text
}

// Erklaert, was man hoeren soll -- und was nicht. Ohne den letzten Satz wuerde die fehlende
// Vorne/Hinten-Unterscheidung fuer einen Fehler gehalten, obwohl sie eine bewusste Grenze des
// Modells ist (s. audio::spatial-Modulkommentar).
// Fest verdrahtet und NICHT der eingestellte Tippton: der Hoertest soll immer dasselbe
// Messsignal spielen, sonst misst man beim Vergleichen zweier Geraete zwei verschiedene Dinge.
pub(crate) const ORBIT_TEST_SOUND: &str = "kenney-interface/click_001";

pub(crate) const MASTER_VOLUME_TOOLTIP: &str =
    "Lautstärke aller Geräusche: Weltgeräusche (Schritte, Blöcke, Kreaturen, Explosionen), die der Server schickt, und die Klänge der Oberfläche.";

pub(crate) const SPATIAL_TEST_TOOLTIP: &str =
    "Lässt einen Klick in drei Sekunden einmal um den Kopf wandern: vorne, rechts, hinten, links. Damit lässt sich prüfen, ob die Ortung ankommt -- der Klick erreicht das nahe Ohr bis zu 0,66 ms früher und das ferne leiser und dumpfer. Am deutlichsten mit KOPFHÖRERN; über Lautsprecher hört man vor allem die Lautstärke. Vorne und hinten klingen gleich, das ist so gewollt (dafür bräuchte es gemessene Ohrmuschel-Daten).";

// Slider-Tick (UiSound::Slider): so viele Halbtoene steigt er von links nach rechts ueber die
// ganze Bahn -- wie im Sound-Baukasten eingestellt.
pub(crate) const SLIDER_TICK_RISE_SEMITONES: f32 = 5.5;

// Deterministischer Pseudozufallswert in -1..1 aus einem fortlaufenden Seed -- derselbe
// xorshift-Multiply-Ansatz wie audio::Cue::varied, damit zwei aufeinanderfolgende Tipptoene
// nicht denselben "zufaelligen" Wert bekommen.
pub(crate) fn zufallswert(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(2_654_435_761);
    x ^= x >> 13;
    x = x.wrapping_mul(1_274_126_177);
    ((x >> 8) as f32 / 8_388_608.0) - 1.0
}

// FPS-Limit: intern der Wert in Hz, NICHT (mehr) ein Index in eine feste Liste -- seit man
// die Zahl anklicken und frei eintippen kann, muss jeder Wert darstellbar sein (90, 165, 165
// standen nie in der Liste). Werte UEBER der Monitor-Refreshrate sind nicht sinnlos: wir
// praesentieren mit PresentMode::Immediate, also ohne auf den VBlank zu warten. Oefter zu
// rendern heisst dann, dass der Monitor beim Auslesen frischere Daten vorfindet -- weniger
// Latenz, dafuer Tearing und mehr Stromverbrauch.
//
// Zwei Sonderwerte, bewusst an den numerischen Enden, damit die Pfeiltasten ohne Sonderfall
// einfach auf-/absteigend durch FPS_LIMIT_STEPS laufen koennen:
pub(crate) const FPS_LIMIT_MONITOR: u32 = 0; // dem Monitor folgen (Standard)
pub(crate) const FPS_LIMIT_UNLIMITED: u32 = u32::MAX; // gar kein Deckel -- per "-1" eingebbar
                                           // Voreinstellungen, durch die die Pfeile schrittweise laufen. Ein frei getippter
                                           // Zwischenwert (z.B. 90) ist voellig erlaubt; der naechste Pfeilklick springt dann auf die
                                           // naechste Stufe darueber bzw. darunter.
pub(crate) const FPS_LIMIT_STEPS: [u32; 8] = [FPS_LIMIT_MONITOR, 30, 60, 75, 120, 144, 240, FPS_LIMIT_UNLIMITED];
// Rechtes Ende des Reglers (getippte Werte bleiben, wie sie sind).
pub(crate) const FPS_LIMIT_TYPED_MAX: u32 = 10_000;
// Die frueher hier stehende Liste, in der settings.conf nur der INDEX gespeichert wurde --
// wird beim Laden einer alten Datei noch gebraucht, um daraus den echten Hz-Wert zu machen
// (s. Settings::parse, Schluessel "fps_limit"). Neue Dateien schreiben "fps_limit_hz".
pub(crate) const LEGACY_FPS_LIMIT_OPTIONS: [Option<u32>; 7] = [None, Some(30), Some(60), Some(75), Some(120), Some(144), Some(240)];

pub(crate) fn fps_limit_label(value: u32) -> String {
    match value {
        FPS_LIMIT_MONITOR => "Monitor".to_string(),
        FPS_LIMIT_UNLIMITED => "unbegrenzt".to_string(),
        v => format!("{v}"),
    }
}

// Die naechste Stufe ueber bzw. unter `value` -- None, wenn es in die Richtung keine mehr
// gibt (dann blendet der jeweilige Pfeil ab). Weil die beiden Sonderwerte numerisch die
// Enden bilden, reicht ein schlichter Groessenvergleich.
pub(crate) fn fps_limit_step(value: u32, up: bool) -> Option<u32> {
    if up {
        FPS_LIMIT_STEPS.iter().copied().find(|&s| s > value)
    } else {
        FPS_LIMIT_STEPS.iter().copied().rev().find(|&s| s < value)
    }
}

// CPU-Render-Ahead-Limit. 1..4 setzen wgpu::SurfaceConfiguration::desired_maximum_frame_
// latency direkt (die Swapchain entscheidet, clamped bei Bedarf selbst). 0 ist ein Sonderfall
// (Sodiums "CPU Render-Ahead Limit: 0 frame(s)"-Nachbau, s. GpuState::fence_wait_zero): laut
// wgpu-Doku wird eine 0 im Swapchain-Parameter selbst "generally not supported and always
// clamped to a higher value" -- deshalb bekommt die Swapchain dabei weiterhin 1, und die
// eigentliche Drosselung passiert per expliziter Fence-Wait (device.poll(WaitForSubmission
// Index)) am Anfang jedes Frames, bevor das naechste ueberhaupt anfaengt. Obergrenze 4
// grosszuegig ueber den ueblichen Treiber-Standardwerten (meist 2-3) fuer den seltenen Fall,
// dass ein hoeherer Wert Ruckler unter Last glaettet -- Latenz-Kosten sind dem Nutzer per
// Tooltip klar gemacht.
pub(crate) const FRAME_LATENCY_MIN: u32 = 0;
pub(crate) const FRAME_LATENCY_MAX: u32 = 4;
// Knapp unter 90 Grad, um Gimbal-Flip beim senkrechten Blick zu vermeiden.
pub(crate) const MAX_PITCH: f32 = 89.0; // Grad -- s. camera.rs, Kamera speichert jetzt in Grad statt Radiant

// Historisch die Job-Vergabe pro Frame gedeckelt (s. Settings::mesh_columns_per_frame) --
// bewusst NICHT mehr fuer diesen Zweck genutzt (s. App::mesh_pending_columns-Kommentar):
// Konstanten bleiben nur noch fuer den Wertebereich der (weiterhin sichtbaren, aber
// wirkungslosen) Settings-Zeile stehen.
pub(crate) const DEFAULT_MESH_COLUMNS_PER_FRAME: usize = 6;
pub(crate) const MESH_COLUMNS_MAX: usize = 32;

// Client-seitige Render-Limiter (s. App::enforce_render_limits) -- zusaetzlich zu, nicht
// statt protocol::REQUESTED_VIEW_DISTANCE: der Server schickt weiterhin, was er bei dieser
// Anfrage hergibt (Vanilla hoechstens 32), diese beiden Regler entscheiden nur, wie viele davon TATSAECHLICH GEMESHT UND
// GEZEICHNET werden (Block-Daten bleiben trotzdem geladen, kein Loch in der Welt, wenn man
// sich wieder naehert).
// Obergrenze der Distanz = REQUESTED_VIEW_DISTANCE: mehr zu erlauben waere wirkungslos, der
// Server liefert ohnehin nicht mehr.
pub(crate) const MAX_RENDER_DISTANCE_MIN: u32 = 1;
pub(crate) const MAX_RENDER_DISTANCE_MAX: u32 = protocol::packets::REQUESTED_VIEW_DISTANCE as u32;

// Worker-Anzahl fuer den dedizierten Mesh-Pool (s. mesh_pool::MeshPool) -- Obergrenze
// grosszuegig ueber ueblichen Kernzahlen, Standard an die tatsaechliche CPU angelehnt statt
// eine feste Zahl zu raten.
pub(crate) const CHUNK_UPDATE_THREADS_MIN: usize = 1;
pub(crate) const CHUNK_UPDATE_THREADS_MAX: usize = 32;
pub(crate) fn default_chunk_update_threads() -> usize {
    std::thread::available_parallelism().map_or(4, std::num::NonZero::get).clamp(CHUNK_UPDATE_THREADS_MIN, CHUNK_UPDATE_THREADS_MAX)
}

// Frame-Metrik ueber ein gleitendes Ein-Sekunden-Fenster.
//
// Die Bildrate allein sagt bei diesem Client wenig: die Frames sind gedeckelt (s.
// next_frame_time), sie klebt also im Normalfall am Limit und verraet nichts ueber die Reserve.
// Aussagekraeftig ist die ARBEITSZEIT pro Frame -- und davon das Maximum, weil einzelne
// Ausreisser (z.B. Meshing, ein Back-Schritt) im Mittelwert untergehen, auf dem Bildschirm
// aber als Ruckler ankommen.
//
// Arbeit ist das Laengere von CPU und GPU, denn beide laufen nebeneinander und die langsamere
// gibt den Takt vor:
// - CPU: die ganze Zeit seit dem vorigen Frame -- Schleife (Pakete, Meshing, Ticks),
//   Frame-Aufbau, Encoding -- ohne Schlafen bis zum naechsten Termin und ohne Warten auf
//   GPU, Swapchain oder Riss-Termin. Frueher zaehlte nur das Encoding: Meshing im selben
//   Frame liess das Bild ausfallen, ohne dass die Anzeige ausschlug.
// - GPU: Zeitstempel vom ersten bis zum letzten Pass, das Backen der Lichtkarten
//   eingeschlossen (s. gpu_timer). Die CPU sieht davon im Zero-Modus nur, was ueber ihren
//   Schlaf hinausgeht -- 10 ms GPU bei 3 ms CPU erschienen dort als 7,7 ms.
#[derive(Default)]
pub(crate) struct FrameStats {
    pub(crate) samples: VecDeque<(Instant, f32)>,
    // GPU-Zeiten, ein, zwei Frames spaeter eingetroffen (s. record_gpu).
    pub(crate) gpu: VecDeque<(Instant, f32)>,
    pub(crate) last_frame: Option<Instant>,
    // Seit dem letzten Frame verschlafen (s. App::loop_idle_since).
    pub(crate) slept_ms: f32,
    // Ueber die ganze Sitzung, nur Frames in der Welt: zum Vergleichen zweier Builds, weil der
    // HUD-Wert (Maximum einer Sekunde) dafuer zu stark schwankt. Beim Beenden ins Log.
    pub(crate) session_cpu: WorkHistogram,
    pub(crate) session_gpu: WorkHistogram,
}

// Verteilung von Frame-Zeiten in 0,1-ms-Faechern bis 100 ms, darueber im letzten Fach.
pub(crate) struct WorkHistogram {
    pub(crate) bins: Box<[u32; 1001]>,
    pub(crate) count: u64,
    pub(crate) sum_ms: f64,
    pub(crate) max_ms: f32,
}

impl Default for WorkHistogram {
    fn default() -> Self {
        Self { bins: Box::new([0; 1001]), count: 0, sum_ms: 0.0, max_ms: 0.0 }
    }
}

impl WorkHistogram {
    pub(crate) fn add(&mut self, ms: f32) {
        let fach = ((ms * 10.0).round() as usize).min(1000);
        self.bins[fach] += 1;
        self.count += 1;
        self.sum_ms += f64::from(ms);
        self.max_ms = self.max_ms.max(ms);
    }

    // Kleinste Zeit, unter oder bei der der Anteil `q` aller Frames liegt.
    pub(crate) fn quantile(&self, q: f64) -> f32 {
        let ziel = (self.count as f64 * q).ceil() as u64;
        let mut summe = 0u64;
        for (fach, &n) in self.bins.iter().enumerate() {
            summe += u64::from(n);
            if summe >= ziel.max(1) {
                return fach as f32 / 10.0;
            }
        }
        self.max_ms
    }

    pub(crate) fn summary(&self) -> String {
        if self.count == 0 {
            return "keine Frames".to_string();
        }
        format!(
            "{} Frames, mittel {:.2} ms, p50 {:.1}, p90 {:.1}, p99 {:.1}, p99.9 {:.1}, max {:.1} ms",
            self.count,
            self.sum_ms / self.count as f64,
            self.quantile(0.5),
            self.quantile(0.9),
            self.quantile(0.99),
            self.quantile(0.999),
            self.max_ms
        )
    }
}

impl FrameStats {
    pub(crate) fn sleep(&mut self, ms: f32) {
        self.slept_ms += ms;
    }

    // Am Ende eines gezeigten Frames, mit der Wartezeit darin (s. FrameTiming::idle_ms).
    pub(crate) fn record(&mut self, now: Instant, idle_ms: f32) -> f32 {
        let cpu_ms = match self.last_frame {
            Some(last) => (now.duration_since(last).as_secs_f32() * 1000.0 - self.slept_ms - idle_ms).max(0.0),
            None => 0.0,
        };
        self.last_frame = Some(now);
        self.slept_ms = 0.0;
        self.samples.push_back((now, cpu_ms));
        Self::trim(&mut self.samples, now);
        cpu_ms
    }

    pub(crate) fn record_gpu(&mut self, now: Instant, ms: f32) {
        self.gpu.push_back((now, ms));
        Self::trim(&mut self.gpu, now);
    }

    pub(crate) fn trim(samples: &mut VecDeque<(Instant, f32)>, now: Instant) {
        while samples.front().is_some_and(|&(t, _)| now.duration_since(t) > HUD_PERF_WINDOW) {
            samples.pop_front();
        }
    }

    // Frames je Sekunde, aus dem halben Sekunde langen Fenster hochgerechnet.
    pub(crate) fn fps(&self) -> usize {
        (self.samples.len() as f32 / HUD_PERF_WINDOW.as_secs_f32()).round() as usize
    }

    pub(crate) fn max_ms(&self) -> f32 {
        self.samples.iter().chain(&self.gpu).map(|&(_, ms)| ms).fold(0.0, f32::max)
    }
}

// Wie FrameStats, aber ohne Wert pro Sample -- zaehlt nur, wie viele Entity-Positions-
// Pakete (Move Entity Pos[/Rot], Entity Position Sync, Teleport Entity) im letzten
// HUD_WINDOW (1s) angekommen sind. Fuer die F3-Frage "kommen die wirklich mit 20 Hz an,
// oder wirkt das nur durch die (inzwischen entfernte) Interpolation glatter als es ist".
#[derive(Default)]
pub(crate) struct EntityUpdateStats {
    pub(crate) samples: VecDeque<Instant>,
}

impl EntityUpdateStats {
    pub(crate) fn record(&mut self, now: Instant) {
        self.samples.push_back(now);
        while self.samples.front().is_some_and(|&t| now.duration_since(t) > HUD_WINDOW) {
            self.samples.pop_front();
        }
    }

    pub(crate) fn per_second(&self) -> usize {
        self.samples.len()
    }
}
