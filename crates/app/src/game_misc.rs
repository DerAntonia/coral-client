// Tick-Takt, Debug-Logs, Angriff und Ausruesten, Schild- und Blockzustand, Meshing-Ergebnisse,
// Inventar-Ziehen.

use crate::*;

// Wie lange nach dem Oeffnen des Chats das winit-Duplikat der Oeffnen-Taste noch verworfen
// wird (s. App::swallow_text_key). Ein paar Frames reichen -- winit liefert das Duplikat
// unmittelbar danach. Kurz genug, dass ein absichtlich getipptes Zeichen nicht betroffen ist:
// man muesste dafuer genau denselben Buchstaben innerhalb von 200 ms nach dem Oeffnen tippen.
// Wann der naechste Physik-Tick faellig ist: das Rest-Zeitguthaben bis zum vollen Tick,
// gerechnet ab dem letzten Abtastzeitpunkt. `accum` ist immer kleiner als ein Tick (die
// Tick-Schleife laeuft, bis das gilt), der Abstand also positiv -- die saturierende Rechnung
// ist nur die Absicherung gegen einen unerwarteten Wert.
pub(crate) fn tick_deadline(last_update: Instant, accum: f32) -> Instant {
    let rest = (physics::TICK_DT - accum).max(0.0);
    last_update + Duration::from_secs_f32(rest)
}

// Der Zeitpunkt, zu dem die Fensterschleife spaetestens wieder aufwachen muss: der FRUEHERE
// von Frame-Termin und Tick-Termin.
//
// Das ist der ganze Kern der Tick-Entkopplung. Vorher schlief die Schleife bis zum
// Frame-Termin, und ein dazwischen faelliger Tick wurde erst beim Aufwachen nachgeholt --
// bei 120 FPS also bis zu 8,3 ms zu spaet, und dieser Verzug landet auf jedem
// Bewegungspaket, weil die Pakete im Tick rausgehen.
//
// Zusaetzliche Wachphasen kosten fast nichts: gezeichnet wird weiterhin nur, wenn der
// FRAME-Termin erreicht ist (s. about_to_wait), ein Tick-Aufwachen macht also Physik und
// Pakete, aber kein Bild.
// Bis `at_ns` (MONOTONIC) warten: grob schlafen, den Rest aktiv -- fuer die Riss-Steuerung muss
// der Zeitpunkt genauer sitzen, als ein Schlaf aufwacht.
//
// Linux: Der Grossteil der Schlaf-Verspaetung ist der Timer-Slack des Threads (Standard 50 us,
// der Kernel buendelt damit Weckzeiten). Mit Slack 1 ns gemessen (2026-11-04, Kernel 7.0,
// HZ=1000, Client lief daneben): p50 2 us, p99 12 us, p99.9 <= 31 us zu spaet. Darum nur 100 us
// aktiv statt 1 ms -- das aktive Warten haelt den Kern wach und kostete im Profil 13 % der Samples.
// Der Slack gilt fuer den ganzen Thread, also auch fuer die uebrigen Schlafphasen der
// Hauptschleife (die wachen dadurch ebenfalls puenktlicher auf).
pub(crate) fn wait_until_ns(at_ns: i64) {
    #[cfg(target_os = "linux")]
    const SPIN_NS: i64 = 100_000;
    #[cfg(not(target_os = "linux"))]
    const SPIN_NS: i64 = 1_000_000;
    #[cfg(target_os = "linux")]
    {
        thread_local!(static SLACK_SET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
        if !SLACK_SET.get() {
            SLACK_SET.set(true);
            unsafe { libc::prctl(libc::PR_SET_TIMERSLACK, 1 as libc::c_ulong, 0, 0, 0) };
        }
    }
    loop {
        let rest = at_ns - vblank::monotonic_ns();
        if rest <= 0 {
            return;
        }
        if rest > SPIN_NS {
            std::thread::sleep(Duration::from_nanos((rest - SPIN_NS) as u64));
        } else {
            std::hint::spin_loop();
        }
    }
}

pub(crate) fn next_wake(frame_deadline: Instant, tick_deadline: Option<Instant>) -> Instant {
    match tick_deadline {
        Some(tick) => frame_deadline.min(tick),
        None => frame_deadline,
    }
}

pub(crate) const SWALLOW_TEXT_KEY_WINDOW: Duration = Duration::from_millis(200);

// Doppelklick im Inventar: Vanillas Fenster (Mouse.field_61505 = 250ms).
pub(crate) const INVENTORY_DOUBLE_CLICK: Duration = Duration::from_millis(250);

// Ein laufendes Verteilen-Ziehen: mit vollem Zeiger ueber mehrere Slots ziehen verteilt den
// Stapel auf sie (links gleichmaessig, rechts je ein Stueck). Vanilla sammelt die Slots
// LOKAL und schickt erst beim Loslassen die ganze Kette (HandledScreen.mouseReleased) --
// deshalb steht hier eine Liste und nicht schon ein Paket.
// ScreenHandler.packQuickCraftData: Stufe in den unteren zwei Bits, Taste in den zwei darueber.
// Stufe: 0 = Anfang, 1 = ein Slot, 2 = Ende. Taste: 0 = links, 1 = rechts.
// Die Geschwindigkeit nach einem Server-Teleport -- Vanillas PositionMoveRotation::
// calculateAbsolute, nur der Delta-Teil (Position und Rotation loest der Aufrufer selbst auf):
// mit ROTATE_DELTA wird die alte Geschwindigkeit erst um (alter - neuer Pitch) um X und um
// (alter - neuer Yaw) um Y gedreht. Je Achse gilt dann: DELTA-Bit gesetzt = alte (gedrehte)
// Geschwindigkeit plus Paketwert, sonst nur der Paketwert.
//
// ROTATE_DELTA dreht den mitgenommenen Schwung so mit, dass er relativ zur NEUEN Blickrichtung
// dasselbe bedeutet wie vorher zur alten -- bei der Enderperle ist die Rotation relativ mit 0,
// die Drehung faellt also weg und der Schwung kommt unveraendert an.
//
// Gedreht wird mit MathHelper.sin/cos (Float-Tabelle) wie in Vanillas Vec3d.xRot/yRot, nicht
// mit exaktem f64-Sinus -- derselbe Grund wie in physics::add_move_input.
pub(crate) fn teleport_velocity(alt: DVec3, paket: DVec3, flags: i32, alte_rot_deg: (f32, f32), neue_rot_deg: (f32, f32)) -> DVec3 {
    use protocol::packets::{
        POSITION_FLAG_REL_DELTA_X, POSITION_FLAG_REL_DELTA_Y, POSITION_FLAG_REL_DELTA_Z, POSITION_FLAG_ROTATE_DELTA,
    };
    let mut v = alt;
    if flags & POSITION_FLAG_ROTATE_DELTA != 0 {
        // Vec3d.xRot(f): y' = y*cos + z*sin, z' = z*cos - y*sin
        let pitch_rad = (alte_rot_deg.1 - neue_rot_deg.1).to_radians();
        let (sin_p, cos_p) = (f64::from(physics::mc_sin(f64::from(pitch_rad))), f64::from(physics::mc_cos(f64::from(pitch_rad))));
        v = DVec3::new(v.x, v.y * cos_p + v.z * sin_p, v.z * cos_p - v.y * sin_p);
        // Vec3d.yRot(f): x' = x*cos + z*sin, z' = z*cos - x*sin
        let yaw_rad = (alte_rot_deg.0 - neue_rot_deg.0).to_radians();
        let (sin_y, cos_y) = (f64::from(physics::mc_sin(f64::from(yaw_rad))), f64::from(physics::mc_cos(f64::from(yaw_rad))));
        v = DVec3::new(v.x * cos_y + v.z * sin_y, v.y, v.z * cos_y - v.x * sin_y);
    }
    let achse = |relativ: bool, alt: f64, neu: f64| if relativ { alt + neu } else { neu };
    DVec3::new(
        achse(flags & POSITION_FLAG_REL_DELTA_X != 0, v.x, paket.x),
        achse(flags & POSITION_FLAG_REL_DELTA_Y != 0, v.y, paket.y),
        achse(flags & POSITION_FLAG_REL_DELTA_Z != 0, v.z, paket.z),
    )
}

pub(crate) const fn quick_craft_button(stage: i32, button: i8) -> i8 {
    (stage & 3 | (button as i32 & 3) << 2) as i8
}

// CORAL_INTERACT_DEBUG=1: schreibt jedes gesendete Interaktions- und Tick-Paket in der
// REIHENFOLGE mit, in der es die Leitung verlaesst. Genau darauf kommt es bei GrimACs
// PacketOrderI an: dessen PacketOrderProcessor merkt sich pro Tick, was reinkam, und der Tick
// endet fuer ihn am Bewegungspaket bzw. -- nur wenn in diesem Tick KEINES kam -- am
// ClientTickEnd (Check.isTickPacketIncludingNonMovement, aus Grims Jar). Welche zwei Pakete
// tatsaechlich im selben Grim-Tick landen, laesst sich aus dem Quelltext allein nicht
// ablesen; dieser Mitschnitt zeigt es.
// Diagnose der Weltbeschriftungen (CORAL_LABEL_DEBUG=1): loggt hoechstens einmal pro
// Sekunde JEDE gezeichnete Beschriftung samt Quelle, Entity-Id, Typ, Ankerhoehe und Team.
//
// Gebaut, weil zweimal geraten wurde, woher ein doppelter Name kommt -- die Frage ist aus dem
// Code allein nicht zu beantworten: ob ein zweiter Name vom Spieler-Entity, von einem
// Armorstand oder von einem Text-Display kommt, weiss nur der laufende Server.
pub(crate) fn label_debug_an() -> bool {
    std::env::var_os("CORAL_LABEL_DEBUG").is_some()
}

pub(crate) fn label_debug_log(zeilen: &[String]) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static ZULETZT: AtomicU64 = AtomicU64::new(0);
    let jetzt = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let vorher = ZULETZT.load(Ordering::Relaxed);
    if jetzt == vorher {
        return;
    }
    ZULETZT.store(jetzt, Ordering::Relaxed);
    eprintln!("[labels] {} Beschriftung(en) in diesem Bild:", zeilen.len());
    for z in zeilen {
        eprintln!("[labels]   {z}");
    }
}

pub(crate) fn interaktions_log(was: &str) {
    if std::env::var_os("CORAL_INTERACT_DEBUG").is_some() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        eprintln!("[interact] {}.{:03} {was}", t.as_secs() % 1000, t.subsec_millis());
    }
}

// Wie interaktions_log, zusaetzlich mit der Zielposition. Gebraucht fuer Techniken, bei denen
// es genau darauf ankommt, dass ZWEI Klicks DESSELBEN Ticks DIESELBE Stelle tragen -- der
// Anchor-Airplace ist der Fall (s. tick_item_use). Im Log liegen die Klicks eines Ticks
// zwischen zwei `tick_end`-Zeilen, damit ist beides auf einen Blick ablesbar:
// wie viele Klicks der Tick hatte und ob sie dieselbe Position trugen.
pub(crate) fn interaktions_log_pos(was: &str, pos: (i32, i32, i32)) {
    if std::env::var_os("CORAL_INTERACT_DEBUG").is_some() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        eprintln!("[interact] {}.{:03} {was} @ {} {} {}", t.as_secs() % 1000, t.subsec_millis(), pos.0, pos.1, pos.2);
    }
}

// Vanillas getAttackCooldownProgress(0.5) ueber 0.9 -- die Bedingung, ab der ein Schlag als
// voll aufgeladen gilt und damit den Sprint-Rueckstoss ausloest (s.
// App::apply_attack_slowdown).
//
//   getAttackCooldownProgressPerTick() = 20 / Angriffsgeschwindigkeit
//   getAttackCooldownProgress(0.5)     = clamp((ticksSeitAngriff + 0.5) / obiges, 0, 1)
pub(crate) fn angriff_ist_aufgeladen(ticks_seit_angriff: i32, angriffsgeschwindigkeit: f32) -> bool {
    let ticks_pro_ladung = 20.0 / angriffsgeschwindigkeit;
    let ladung = ((ticks_seit_angriff as f32 + 0.5) / ticks_pro_ladung).clamp(0.0, 1.0);
    ladung > 0.9
}

// Zielhoehe der Hand aus der Angriffs-Aufladung (HeldItemRenderer.updateHeldItems), auf der
// Skala von own_equip_progress (0 = oben, 1 = ganz weg). Vanilla nimmt die Ladung hoch drei:
// nach einem Schlag oder Wechsel faellt die Hand ganz weg und schnellt erst zum Ende der
// Aufladung hoch. Die Ladung ist getHandEquippingProgress(1.0) -- derselbe Zaehler wie beim
// Angriff, einen Tick voraus.
pub(crate) fn equip_target(ticks_seit_angriff: i32, angriffsgeschwindigkeit: f32) -> f32 {
    let ticks_pro_ladung = 20.0 / angriffsgeschwindigkeit;
    let ladung = ((ticks_seit_angriff as f32 + 1.0) / ticks_pro_ladung).clamp(0.0, 1.0);
    1.0 - ladung * ladung * ladung
}

// Ein Tick der Hand in Richtung Ziel, mit Vanillas Schrittweite von hoechstens 0.4.
pub(crate) fn equip_step(aktuell: f32, ziel: f32) -> f32 {
    aktuell + (ziel - aktuell).clamp(-0.4, 0.4)
}

// Was Vanillas interactBlockInternal zurueckgibt, auf die drei Faelle reduziert, die
// `MinecraftClient.doItemUse` unterscheidet (s. App::resolve_block_use/do_item_use): Success
// (ggf. schwingen, fertig), Fail (fertig) und alles andere, das zu interactItem durchfaellt.
//
// Der Unterschied zwischen Fehlgeschlagen und Durchgereicht ist NICHT kosmetisch: nur beim
// Durchreichen schickt Vanilla ein zweites Paket. ActionResult.CONSUME zaehlt dabei zu
// Angenommen (es ist ein `Success` mit SwingSource.NONE) -- deshalb das Feld `schwingt`
// statt zweier Varianten.
pub(crate) enum BlockUse {
    Angenommen { schwingt: bool },
    Fehlgeschlagen,
    Durchgereicht,
}

// Was RespawnAnchorBlock mit einer Benutzung macht (s. App::resolve_anchor_use).
pub(crate) enum AnkerBenutzung {
    KeinAnker,
    // Fertig: angenommen (geladen bzw. CONSUME).
    Ergebnis(BlockUse),
    // PASS aus onUseWithItem: onUse entfaellt, das Item dieser Hand kommt noch dran.
    NurItem,
    // Wie ein gewoehnlicher Block weiter (PASS_TO_DEFAULT ohne Wirkung / onUse-PASS).
    Standard,
}

// Alles, was das Schild zum Zeichnen braucht. Es ist das einzige Item in diesem Client, das
// ueber ein ENTITY-Modell statt ueber sein Item-Modell gezeichnet wird (s.
// world::entity_model::shield_model), und deshalb auch das einzige, das seine
// Halte-Transformationen einzeln vorhalten muss.
//
// Rechts und links stehen getrennt, weil sie sich bei shield.json wirklich unterscheiden
// (thirdperson_righthand [10,6,-4] gegen thirdperson_lefthand [10,6,12]) -- bei fast jedem
// anderen Item darf man die linke aus der rechten spiegeln, hier nicht.
// Welcher ARM haelt gerade ein Schild hoch -- (rechts, links)?
//
// `using_item_right_arm` ist Vanillas isUsingItem()+getActiveHand(): None = benutzt nichts,
// Some(true) = Haupthand (rechter Arm), Some(false) = Nebenhand. Der zweite Teil ist Vanillas
// `stack.getUseAction() == BLOCK`: praktisch das Schild, und nur fuer das hat dieser Client
// ueberhaupt ein Modell. Wer mit einem Bogen oder einem Apfel in der Hand benutzt, blockt
// nicht -- deshalb reicht das Benutzungs-Flag allein nicht.
// Ein einzelnes Teil des Schildmodells als eigenes Modell (s. ShieldRender::plate).
pub(crate) fn shield_teil(name: &str) -> world::Model {
    let mut model = world::entity_model::shield_model();
    model.parts.retain(|p| p.name == name);
    model
}

pub(crate) fn blocking_arms(using_item_right_arm: Option<bool>, hands: (Option<world::HeldItem>, Option<world::HeldItem>)) -> (bool, bool) {
    let Some(rechts) = using_item_right_arm else { return (false, false) };
    let item = if rechts { hands.0 } else { hands.1 };
    // ItemUseAnimation BLOCK: das Schild, aber auch alles, dem ein Server blocks_attacks gibt.
    let schild = item.is_some_and(|i| world::entity::held_blocks_attacks(&i).is_some());
    (schild && rechts, schild && !rechts)
}


// Das Schild als Inventar-Icon: dasselbe Entity-Modell wie in der Hand, mit der Einbettung, die
// Vanilla jedem Sondermodell gibt (s. App::emit_shield: translate(-0.5), scale(1, -1, -1)).
// Platte und Griff getrennt, weil ItemIcons die Aussennormalen je Kasten bestimmt.
pub(crate) fn set_shield_icon(icons: &mut ItemIcons, shield: &ShieldRender, entity_atlas: &assets::EntityAtlas) {
    use world::entity_model::Affine;
    let tex = entity_atlas.rect_or_missing(shield.model.texture);
    let base = Affine::translation([-0.5, -0.5, -0.5]).then(Affine::scale([1.0, -1.0, -1.0]));
    let boxes = [&shield.plate, &shield.handle].map(|part| {
        let mut out = Vec::new();
        world::emit_entity_model(part, &world::Pose::new(), base, tex, 1.0, &mut out);
        out
    });
    icons.set_entity_model("shield", &boxes);
}

// Truhen, Shulkerkisten und Kupfergolem-Statuen als Inventar-Icons: Vanilla zeichnet sie ueber
// ein Sondermodell ("type": "special"), also mit dem Entity-Modell statt eines Item-Bilds.
// Ohne das stand dort nur die Partikeltextur als Wuerfel. Einbettung wie bei jedem
// Sondermodell (translate -0.5), danach die Haltung "gui" aus dem Basis-Modell des Items.
pub(crate) fn set_special_icons(icons: &mut ItemIcons, entity_atlas: &assets::EntityAtlas, installation: &mut assets::MinecraftInstallation) {
    use world::entity_model::{self as em, Affine};
    let base = Affine::translation([-0.5, -0.5, -0.5]);
    for id in 0..world::item_registry::item_count() as u32 {
        let Some(name) = world::item_registry::item_name(id) else { continue };
        let Some((kind, texture)) = held_item::special_item(name) else { continue };
        let tex = entity_atlas.rect_or_missing(&texture);
        let (boxes, gui) = match kind {
            held_item::SpecialKind::Chest => {
                (em::emit_boxes(em::chest_model(world::ChestHalf::Single), &em::chest_pose(0.0), base, tex), assets::item_named_display(installation, name, "gui"))
            }
            held_item::SpecialKind::ShulkerBox => (
                em::emit_boxes(em::shulker_box_model(), &em::shulker_box_pose(0.0), base.then(em::shulker_box_transform([0.0; 3], "up")), tex),
                assets::item_named_display(installation, name, "gui"),
            ),
            // Wie Vanilla: nach dem -0.5 die Transformation aus items/copper_golem_statue.json,
            // dann die Statuen-Haltung. Ohne beides zeigte das Icon den Ruecken und sass gut 3
            // Pixel zu hoch (s. Test kupfergolem_statue_steht_aufrecht). Gezeigt wird die
            // Grundpose (Stehen); die anderen haengen am Blockzustand des Stapels.
            // Im Inventar das flache Bild (items/trident.json, Fall "gui").
            held_item::SpecialKind::Trident => continue,
            held_item::SpecialKind::CopperGolemStatue => {
                let Some(model) = world::model_data::layer_model("copper_golem#main") else { continue };
                let gui = assets::model_hand_display(installation, held_item::STATUE_TEMPLATE, "gui");
                (em::emit_boxes(model, &em::copper_golem_statue_pose(model), base.then(held_item::statue_item_transform()), tex), gui)
            }
        };
        icons.set_entity_model_gui(name, &boxes, gui);
    }
}

pub(crate) struct ShieldRender {
    pub(crate) model: world::Model,
    // Dasselbe Modell in seine zwei Teile zerlegt: Vanilla gibt den Glint NUR der Platte
    // (ShieldModelRenderer: der Griff geht mit hasGlint = false in die Queue).
    pub(crate) plate: world::Model,
    pub(crate) handle: world::Model,
    // [rechte Hand, linke Hand], je fuer die dritte und die eigene Ansicht.
    pub(crate) third_person: [assets::ItemDisplay; 2],
    pub(crate) first_person: [assets::ItemDisplay; 2],
    // Dieselben vier Eintraege fuer das HOCHGERISSENE Schild (item/shield_blocking). Das
    // Hochreissen steckt fast ganz in diesen Zahlen, nicht im Arm: in der Ego-Ansicht
    // verschiebt Vanilla ueberhaupt nichts extra, es zeichnet schlicht ein anderes Modell
    // (HeldItemRenderer, Zweig BLOCK: die Verschiebung gilt nur fuer Nicht-Schilde).
    pub(crate) third_person_blocking: [assets::ItemDisplay; 2],
    pub(crate) first_person_blocking: [assets::ItemDisplay; 2],
}

// Eine noch nicht bestaetigte Block-Platzierung (s. App::predict_block_place). Entspricht
// Vanillas PendingUpdateManager.PendingUpdate.
#[derive(Clone)]
pub(crate) struct PendingBlock {
    pub(crate) pos: (i32, i32, i32),
    // Wann die Vorhersage gestellt wurde -- daran haengt die Vorhersage-Toleranz: bis
    // Settings::ghost_tolerance_ms wird der Block wie ein echter gezeichnet, danach
    // halbdurchsichtig (s. ghost_vertices).
    pub(crate) placed_at: std::time::Instant,
    // Sequenznummer des ausloesenden USE_ITEM_ON -- der Block Changed Ack nennt sie zurueck.
    pub(crate) sequence: i32,
    // Was beim Ack in die echten Chunk-Daten geschrieben wird: anfangs der Zustand VOR der
    // Platzierung (also die Ruecknahme), ersetzt durch den Serverzustand, sobald dessen
    // Block-Update fuer diese Stelle eintrifft (s. defer_block_update).
    pub(crate) aufloesung: u32,
    // Hat die Vorhersage hier einen Block ENTFERNT (abgebaut), steht hier, welcher es war --
    // der Ghostblock-Indikator zeigt ihn halbdurchsichtig, solange der Server schweigt.
    pub(crate) entfernt: Option<u32>,
}

// Ein bestaetigter Geisterblock, der noch auf "seinen" Chunk-Mesh wartet.
//
// Der Geisterblock-Indikator darf beim Bestaetigen durch den Server nicht flackern. Ursache war eine Luecke, kein Ueberblenden: beim Ack verschwand der
// Geisterblock SOFORT, der echte Block erschien aber erst, wenn der Mesher seine Spalte neu
// gebaut und hochgeladen hatte -- das laeuft auf dem Worker-Pool und ist auf
// mesh_columns_per_frame gedeckelt, dauert also mehrere Frames. Dazwischen war an der Stelle
// schlicht NICHTS zu sehen, und genau dieses kurze Loch liest sich als Flackern.
pub(crate) struct GhostAwaitingMesh {
    pub(crate) pos: (i32, i32, i32),
    // Spalte, deren Mesh den Block ab jetzt zeichnet.
    pub(crate) column: (i32, i32),
    // Notbremse: kommt der Mesh nie (Spalte inzwischen entladen, Server-Reset), verschwindet
    // der Geist trotzdem. Lieber ein Loch als ein Block, der nie wieder weggeht.
    pub(crate) deadline: std::time::Instant,
}

// Pfeil-Modul, Rueckmeldung beim Spannen ohne Munition (Settings::arrow_empty_feedback).
// Ist die Farbe eines Effekts zu dunkel fuer Text auf dunklem Grund? Wahrgenommene Helligkeit
// (Rec. 601 auf den sRGB-Werten) unter gut einem Drittel: Absorption (0.31), Schwaeche,
// Bergbaumuedigkeit, Dunkelheit, Blindheit, Uebelkeit, Boeses Omen. Dazu Wither (0.40)
// ausdruecklich -- eine hoehere Grenze naehme das kraeftige Rot von Sofortheilung und
// Saettigung (0.39) mit.
pub(crate) fn effect_is_dark(effect: i32) -> bool {
    let rgb = world::potion::effect_color(effect);
    let [r, g, b] = [(rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF].map(|c| c as f32 / 255.0);
    0.299 * r + 0.587 * g + 0.114 * b < 0.35 || protocol::packets::mob_effect_name(effect) == "wither"
}

pub(crate) const ARROW_EMPTY_FEEDBACK_NAMES: [&str; 3] = ["Aus", "0 blinkt", "Wackeln"];
pub(crate) const ARROW_EMPTY_FEEDBACK_BLINK: u8 = 1;
pub(crate) const ARROW_EMPTY_FEEDBACK_SHAKE: u8 = 2;
// So lange blinkt die 0 bzw. wackelt das Fadenkreuz nach dem letzten Versuch.
pub(crate) const ARROW_EMPTY_BLINK_SECS: f32 = 0.8;
pub(crate) const ARROW_EMPTY_SHAKE_SECS: f32 = 0.3;
// Toenung des Pfeil-Icons ohne Munition, linear (sRGB #FF5555 wie EFFECTS_BLINK_RED).
pub(crate) const ARROW_EMPTY_TINT: [f32; 3] = [1.0, 0.0908, 0.0908];

// Autoklicker (Debug): Klicks je Sekunde, Grenzen des Reglers (s. Settings::autoclicker_cps).
pub(crate) const AUTOCLICKER_CPS_DEFAULT: u32 = 12;
pub(crate) const AUTOCLICKER_CPS_MIN: u32 = 1;
pub(crate) const AUTOCLICKER_CPS_MAX: u32 = 30;

// Ghostblock-Indikator: Grenzen und Schrittweite des Reglers (s. Settings::ghost_tolerance_ms).
pub(crate) const GHOST_TOLERANCE_DEFAULT_MS: u32 = 200;
pub(crate) const GHOST_TOLERANCE_MIN_MS: u32 = 0;
pub(crate) const GHOST_TOLERANCE_MAX_MS: u32 = 1000;
pub(crate) const GHOST_TOLERANCE_STEP_MS: u32 = 25;

// No Tool Break: ab so viel verbleibender Haltbarkeit wird das Abbauen gesperrt (Regler im
// Modul-Menue). Ein Block kostet hoechstens 1 -- die Reserve ist Luft fuer die Ticks, in denen
// der Server schon weiter ist als unser Inventar.
pub(crate) const NO_TOOL_BREAK_DEFAULT_RESERVE: u32 = 10;

// Vanillas Ladebildschirm (s. App::vanilla_level_ready): 30 s Hoechstdauer, und der Abschnitt
// unter dem Spieler gilt ab 30 % Einblendung als fertig -- bei Vanillas Vorgabe von 0.75 s
// Chunk-Fade (GameOptions.chunkFade) nach 225 ms.
pub(crate) const VANILLA_LOAD_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const VANILLA_CHUNK_FADE_READY: Duration = Duration::from_millis(225);
pub(crate) const NO_TOOL_BREAK_MIN_RESERVE: u32 = 1;
pub(crate) const NO_TOOL_BREAK_MAX_RESERVE: u32 = 100;
// Hoechstens so oft die Benachrichtigung -- bei gehaltener Taste kaeme sie sonst jeden Tick.
pub(crate) const NO_TOOL_BREAK_NOTIFY_EVERY: Duration = Duration::from_secs(3);
// Vanillas Haltbarkeit der Werkzeugstufen (ToolMaterial.DIAMOND / NETHERITE). max_damage
// steht nur im Stack, wenn der Server davon abweicht (s. protocol::item::ItemStack).
pub(crate) const DIAMOND_TOOL_DURABILITY: i32 = 1561;
pub(crate) const NETHERITE_TOOL_DURABILITY: i32 = 2031;

// Wie lange ein bestaetigter Geisterblock hoechstens auf seinen Mesh wartet.
pub(crate) const GHOST_MESH_GRACE: Duration = Duration::from_millis(500);
// So lange blendet nach einer Platzierung die Eckabdunklung um den neuen Block ein (s. AoFade):
// der Block steht sofort, sein Schatten kommt mit dem neuen Spalten-Mesh und springt nicht.
pub(crate) const AO_FADE: Duration = Duration::from_millis(150);
// So lange bleibt eine bestaetigte Platzierung Quelle fuers Einblenden -- laenger braucht
// kein Mesh-Job (die Notbremse fuer Geister ist GHOST_MESH_GRACE).
pub(crate) const AO_FADE_SOURCE_TTL: Duration = Duration::from_secs(1);

// Ein fertig gemeshtes Spaltenergebnis aus dem Mesh-Pool (s. mesh_pending_columns).
pub(crate) struct MeshResult {
    pub(crate) key: (i32, i32),
    // Opakes Gelaende, nach Richtung und Section geordnet (s. world::group_by_direction).
    pub(crate) opaque: Vec<world::TerrainVertex>,
    pub(crate) groups: [world::FaceGroup; world::FACE_GROUPS],
    pub(crate) runs: Vec<world::SectionRun>,
    // Je Section die Sichtverbindungen ihrer Seiten (s. world::visibility).
    pub(crate) visibility: Vec<world::visibility::SectionVis>,
    // Je Section die sichtdichten Zellen, fuers Sichtlinien-Culling (s. render::sight).
    pub(crate) opacity: std::sync::Arc<[world::visibility::SectionOpacity]>,
    pub(crate) min_y: i32,
    pub(crate) translucent: Vec<world::TerrainVertex>,
    pub(crate) fade_quads: Vec<world::TerrainFadeQuad>,
    // Die Platzierungen, deren Schatten fade_quads einblendet (s. App::ao_fade_sources).
    pub(crate) fade_sources: Vec<(i32, i32, i32)>,
    // Shader-Pfad: Blocklicht der Spalte zum Nachschlagen (s. App::column_light).
    pub(crate) block_light: Option<world::light::ColumnBlockLight>,
    // Flickbar gebaut (s. world::layout_patchable): je Platz im opaken Puffer der Block.
    pub(crate) quad_block: Option<Vec<u32>>,
}

// Was ein Mesh-Auftrag liefert: die ganze Spalte, oder nur die ausgetauschten Flaechen rund um
// geaenderte Bloecke (Flicken, s. render::BlockRenderer::patch_column).
pub(crate) enum MeshOutcome {
    Full(Box<MeshResult>),
    Patch {
        key: (i32, i32),
        // Bloecke, deren alte Flaechen verschwinden (sortierte Kennungen, s. world::block_id).
        removed: Vec<u32>,
        added: Vec<world::PlacedQuad>,
        visibility: Vec<world::visibility::SectionVis>,
        opacity: std::sync::Arc<[world::visibility::SectionOpacity]>,
        min_y: i32,
    },
    // Flicken geht hier nicht (Wasser oder Glas im Spiel): die Spalte ganz neu bauen.
    PatchFailed { key: (i32, i32) },
}

// Eine Spalte, in der sich schon einmal einzelne Bloecke geaendert haben: ihre Meshes werden
// flickbar gebaut. `sections`: wo freie Plaetze fuer JEDE Richtung vorgehalten werden (Bit n =
// Section n). `last_patch`: seit wann sie nur geflickt ist -- nach PATCH_SETTLE Ruhe wird sie
// einmal ganz neu gebaut.
#[derive(Default)]
pub(crate) struct PatchColumn {
    pub(crate) sections: u64,
    pub(crate) last_patch: Option<std::time::Instant>,
}

pub(crate) const PATCH_SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

// Messung Neu-Meshen (s. App::log_remesh).
pub(crate) struct RemeshLog {
    pub(crate) seit: std::time::Instant,
    pub(crate) jobs: u32,
    pub(crate) verschoben: u32,
    pub(crate) geflickt: u32,
    pub(crate) flick_rueckfall: u32,
}

impl RemeshLog {
    pub(crate) fn neu() -> Self {
        Self { seit: std::time::Instant::now(), jobs: 0, verschoben: 0, geflickt: 0, flick_rueckfall: 0 }
    }
}

// Ein laufendes Einblenden der Eckabdunklung in einer Spalte: die betroffenen Quads mit ihren
// Vertices vorher/nachher (s. world::mesh_column_ao_fading).
pub(crate) struct AoFade {
    pub(crate) column: (i32, i32),
    pub(crate) quads: Vec<world::TerrainFadeQuad>,
    // Beginn des Einblendens: der Moment, in dem das erste Mesh mit dieser Platzierung kam
    // (s. App::ao_fade_sources). Ein spaeterer Neubau der Spalte setzt an derselben Stelle
    // fort statt von vorn zu beginnen.
    pub(crate) start: Instant,
}

// Eine abgeschickte Verteil-Kette, deren Ergebnis noch aussteht (s. inventory_release).
// Weicht unsere Revision ab, antwortet der Server auf JEDES Kettenpaket mit einem vollen
// Container-Abgleich -- die ersten davon noch mit dem unverteilten Stand, erst der letzte mit
// dem Ergebnis. Ohne diese Merkliste loeschten die Zwischenstaende die Vorschau, und die
// verteilten Items waren kurz unsichtbar.
pub(crate) struct QuickCraftPending {
    // Die Vorschau: Slot und Inhalt nach dem Verteilen, dazu der Zeiger danach.
    pub(crate) slots: Vec<(usize, Option<protocol::item::ItemStack>)>,
    pub(crate) cursor: Option<protocol::item::ItemStack>,
    // So viele Abgleiche koennen noch kommen (einer je Kettenpaket); der letzte gilt.
    pub(crate) remaining: usize,
    pub(crate) since: Instant,
}

// Laenger als das haelt keine Vorschau gegen den Server -- danach gilt, was er schickt.
pub(crate) const QUICK_CRAFT_PENDING_MAX: Duration = Duration::from_secs(1);

pub(crate) struct InventoryDrag {
    // 0 = links (gleichmaessig aufteilen), 1 = rechts (je ein Stueck).
    pub(crate) button: i8,
    // Ueberfahrene Slots samt dem, was dort VOR dem Ziehen lag. Die Vorschau wird bei jedem
    // neuen Slot komplett neu daraus gerechnet (s. inventory::drag_preview).
    pub(crate) slots: Vec<(usize, Option<protocol::item::ItemStack>)>,
    // Der Stapel am Zeiger, als das Ziehen begann.
    pub(crate) cursor_before: Option<protocol::item::ItemStack>,
    // War "Stapel verschieben" beim DRUECKEN gehalten? Das Loslassen fuehrt den Klick aus, der
    // beim Druecken schon angezeigt wurde -- erst danach gedrueckt hob es sonst das eben
    // abgelegte Item wieder auf.
    pub(crate) move_stack: bool,
}

// Fokussiertes Eingabefeld im Server-Formular -- hebt sich vom unfokussierten ab, ohne wie
// ein Button auszusehen.
// Farbe der Trefferflaechen ueber den Server-Karten: voellig durchsichtig. styled_button
// zeichnet dafuer nichts (s. dort), die Karte selbst kommt aus build_server_panel.
pub(crate) const SERVER_CARD_MARKER: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
// Tastenbelegung, die im selben Kontext noch eine andere Aktion ausloest (Vanilla: rot).
// Abgeblendet = Aktion gerade nicht moeglich (Klick wird ignoriert).
// HUD-Editor: dezente Rasterlinien, Griff-Rahmen um ein Element (heller beim Ziehen).
pub(crate) const COLOR_GRID_LINE: [f32; 4] = [1.0, 1.0, 1.0, 0.08];
pub(crate) const COLOR_HUD_HANDLE: [f32; 4] = [0.30, 0.55, 0.95, 0.35];
pub(crate) const COLOR_HUD_HANDLE_ACTIVE: [f32; 4] = [0.35, 0.70, 1.0, 0.6];
// Gestorben-Screen: halbtransparenter roter Schleier + heller roter Titel.
pub(crate) const COLOR_DEATH_OVERLAY: [f32; 4] = [0.45, 0.0, 0.0, 0.55];
pub(crate) const COLOR_DEATH_TITLE: [f32; 4] = [1.0, 0.85, 0.85, 1.0];
// Angedockt (folgt dem Log) vs. abgedockt (haengt an einer festen Stelle) -- die
// Thumb-Farbe ist die einzige Anzeige dafuer, in welchem der beiden Zustaende man ist.
// Zeiger-Form wie osu!lazers Menue-Zeiger (an menu-cursor.png aus ppy/osu-resources
// abgemessen, in dessen Einheiten): ein Dreieck mit Kerbe, Spitze oben links. Die vier Punkte
// sind die INNERE Kante der Kontur (Spitze, rechter Fluegel, Kerbe, unterer Fluegel); die
// sichtbare Form reicht ringsum CURSOR_ROUNDING weiter, dadurch sind die Ecken aussen rund.
// Als Formen statt als Bild erzeugt: bleibt bei jeder Aufloesung scharf, keine Textur-Datei.
pub(crate) const CURSOR_SHAPE: [(f32, f32); 4] = [(31.0, 21.0), (275.0, 226.0), (129.0, 238.0), (31.0, 340.0)];
pub(crate) const CURSOR_ROUNDING: f32 = 20.0;
// Ring unter der Kerbe wie in osu, abschaltbar (Settings::cursor_circle): Mitte und Aussenradius.
pub(crate) const CURSOR_CIRCLE_CENTER: (f32, f32) = (180.5, 341.5);
pub(crate) const CURSOR_CIRCLE_RADIUS: f32 = 45.0;
// Konturdicke bei 100 % (Settings::cursor_outline) -- osus Dicke. Sie waechst nach innen, die
// Aussenform bleibt; in Einheiten der Form, waechst also mit der Zeigergroesse.
pub(crate) const CURSOR_OUTLINE: f32 = 20.0;
// Schatten: dieselbe Form, schwarz zu 65 % deckend wie in osu (Nutzer 2026-11-11), nach links
// unten versetzt.
pub(crate) const CURSOR_SHADOW_OFFSET: (f32, f32) = (-10.0, 11.0);
pub(crate) const CURSOR_SHADOW_ALPHA: f32 = 0.65;
// Bildschirmhoehe des Pfeils bei 100 % (etwa die des bisherigen Zeigers); er ist in der
// Vorlage CURSOR_HEIGHT_UNITS hoch.
pub(crate) const CURSOR_HEIGHT_PX: f32 = 24.0;
pub(crate) const CURSOR_HEIGHT_UNITS: f32 = 360.0;

// Weisse Kontur, das Innere schwarz -- seine Deckkraft kommt aus dem Regler
// (Settings::cursor_fill_transparency, 50 % wie osu).
pub(crate) const COLOR_CURSOR_OUTLINE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
pub(crate) const COLOR_CURSOR_FILL: [f32; 3] = [0.0, 0.0, 0.0];
// Wie weit das Innere beim Druecken Richtung Akzentfarbe geht (0 = gar nicht, 1 = ganz) --
// "etwas", nicht voll.
pub(crate) const CURSOR_PRESS_TINT: f32 = 0.55;

// MESSUNG (voruebergehend): Millisekunden seit dem ersten Aufruf, fuer die [flick]-Zeilen.
pub(crate) fn mess_ms() -> u128 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis()
}
