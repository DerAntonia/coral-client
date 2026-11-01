// Statusleisten-Symbole und Effekt-Anzeige.

use crate::*;

// Vordergrund-Herz ueber dem leeren Behaelter; None = dieses Herz ist leer.
pub(crate) fn heart_sprite(health: f32, index: i32) -> Option<&'static str> {
    let voll = (index * 2 + 2) as f32;
    let halb = (index * 2 + 1) as f32;
    if health >= voll {
        Some("hud/heart/full")
    } else if health >= halb {
        Some("hud/heart/half")
    } else {
        None
    }
}

// Ein eigener Statuseffekt, wie ihn die Anzeige braucht (s. App::shown_effects).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShownEffect {
    pub(crate) amplifier: i32,
    // Dauer in Ticks beim Eintreffen, protocol::packets::MOB_EFFECT_INFINITE = unendlich.
    pub(crate) duration: i32,
    pub(crate) flags: u8,
    pub(crate) since: Instant,
}

impl ShownEffect {
    pub(crate) const fn ambient(&self) -> bool {
        self.flags & protocol::packets::MOB_EFFECT_FLAG_AMBIENT != 0
    }

    pub(crate) const fn show_icon(&self) -> bool {
        self.flags & protocol::packets::MOB_EFFECT_FLAG_SHOW_ICON != 0
    }

    // Restdauer in Ticks (-1 = unendlich), heruntergezaehlt seit dem Eintreffen.
    pub(crate) fn remaining(&self, now: Instant) -> i32 {
        if self.duration == protocol::packets::MOB_EFFECT_INFINITE {
            return self.duration;
        }
        let vergangen = (now.saturating_duration_since(self.since).as_secs_f64() * 20.0) as i32;
        (self.duration - vergangen).max(0)
    }

    // Dasselbe ungerundet, zum Sortieren: zwei gleich lange Effekte, die einige Millisekunden
    // auseinander kamen, springen in `remaining` abwechselnd um einen Tick vor -- die
    // Reihenfolge kippte dann von Bild zu Bild hin und her.
    pub(crate) fn remaining_exact(&self, now: Instant) -> f64 {
        if self.duration == protocol::packets::MOB_EFFECT_INFINITE {
            return f64::from(self.duration);
        }
        (f64::from(self.duration) - now.saturating_duration_since(self.since).as_secs_f64() * 20.0).max(0.0)
    }
}

// Ein Effekt fuer die Anzeige: ID, ambient, Staerke, Restdauer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EffectEntry {
    pub(crate) effect: i32,
    pub(crate) ambient: bool,
    pub(crate) amplifier: i32,
    pub(crate) remaining: i32,
    // Restdauer ungerundet, nur zum Sortieren (s. ShownEffect::remaining_exact).
    pub(crate) exact: f64,
    // Dauer beim Eintreffen des Effekts (fuer das dynamische Blinken, s. effect_timer_blink).
    pub(crate) received: i32,
}

// MobEffectInstance.compareTo: erst nicht-ambiente, dann endliche vor unendlichen, dann nach
// Restdauer und Farbe -- ausser beide sind lang (> 32147 Ticks) oder beide ambient, dann nur
// ambient und Farbe.
pub(crate) fn effect_order(a: &EffectEntry, b: &EffectEntry) -> std::cmp::Ordering {
    let farbe = |e: &EffectEntry| world::potion::effect_color(e.effect);
    const GRENZE: i32 = 32147;
    if (a.remaining <= GRENZE || b.remaining <= GRENZE) && !(a.ambient && b.ambient) {
        let unendlich = |e: &EffectEntry| e.remaining == protocol::packets::MOB_EFFECT_INFINITE;
        a.ambient
            .cmp(&b.ambient)
            .then(unendlich(a).cmp(&unendlich(b)))
            .then(a.exact.total_cmp(&b.exact))
            .then(farbe(a).cmp(&farbe(b)))
    } else {
        a.ambient.cmp(&b.ambient).then(farbe(a).cmp(&farbe(b)))
    }
}

// Reihenfolge der detaillierten Anzeige: unendliche Effekte, dann nach Restdauer absteigend;
// gleich lange nach ID, damit sie nicht gegeneinander springen.
pub(crate) fn effect_longest_first(a: &EffectEntry, b: &EffectEntry) -> std::cmp::Ordering {
    let dauer = |e: &EffectEntry| if e.remaining == protocol::packets::MOB_EFFECT_INFINITE { f64::MAX } else { e.exact };
    dauer(b).total_cmp(&dauer(a)).then(a.effect.cmp(&b.effect))
}

// Gui.extractEffects: Deckkraft des Symbols. Endet ein nicht-ambienter Effekt in hoechstens
// 10 Sekunden, blinkt das Symbol und wird dabei immer blasser.
pub(crate) fn effect_icon_alpha(entry: &EffectEntry) -> f32 {
    let d = entry.remaining;
    if entry.ambient || d == protocol::packets::MOB_EFFECT_INFINITE || d > 200 {
        return 1.0;
    }
    let sekunden = 10 - d / 20;
    let a = (d as f32 / 10.0 / 5.0 * 0.5).clamp(0.0, 0.5)
        + (d as f32 * std::f32::consts::PI / 5.0).cos() * (sekunden as f32 / 10.0 * 0.25).clamp(0.0, 0.25);
    a.clamp(0.0, 1.0)
}

// Platzierung der HUD-Effekte (Gui.extractEffects) in GUI-Pixeln relativ zur RECHTEN oberen
// Ecke: `liste` aufsteigend sortiert, gezeichnet wird umgekehrt -- gute Effekte oben von
// rechts nach links, alle anderen eine Reihe tiefer. Rueckgabe: (Eintrag, x, y) je Symbol
// (x negativ) und die Groesse der Anzeige.
pub(crate) fn effect_hud_layout(liste: &[EffectEntry]) -> (Vec<(EffectEntry, f32, f32)>, f32, f32) {
    let (mut gut, mut schlecht) = (0, 0);
    let mut plaetze = Vec::new();
    for e in liste.iter().rev() {
        let (x, y) = if world::potion::effect_beneficial(e.effect) {
            gut += 1;
            (-25.0 * gut as f32, 1.0)
        } else {
            schlecht += 1;
            (-25.0 * schlecht as f32, 27.0)
        };
        plaetze.push((*e, x, y));
    }
    let breite = 25.0 * gut.max(schlecht) as f32;
    let hoehe = if schlecht > 0 { 51.0 } else if gut > 0 { 25.0 } else { 0.0 };
    (plaetze, breite, hoehe)
}

// Detaillierte Effektanzeige: der feste Rand, an dem die Liste beginnt. Jeder weitere Effekt
// kommt auf der anderen Seite dazu -- bei "Unten" also darueber.
pub(crate) const EFFECTS_ALIGNS: [&str; 4] = ["Oben", "Unten", "Links", "Rechts"];
pub(crate) const EFFECTS_ALIGN_BOTTOM: u8 = 1;
pub(crate) const EFFECTS_ALIGN_LEFT: u8 = 2;
pub(crate) const EFFECTS_ALIGN_RIGHT: u8 = 3;
// Wie der Timer zaehlt: Sekunden ("95"), Minuten:Sekunden ("1:35"), Minuten mit Zehnteln
// ("1.5").
pub(crate) const EFFECTS_TIMER_STYLES: [&str; 3] = ["Sekunden", "Min:Sek", "Minuten"];

// Ab wie vielen Restticks der Timer blinkt, wie lange je Farbe, und das Rot dazu (Vanillas
// Textfarbe red).
pub(crate) const EFFECTS_BLINK_TICKS: i32 = 200;
pub(crate) const EFFECTS_BLINK_HALF: i32 = 10;
pub(crate) const EFFECTS_BLINK_RED: u32 = 0xFF5555;
// Wann der Timer blinkt: nie, immer, oder dynamisch -- nur bei Effekten, die mit mehr als
// EFFECTS_BLINK_DYNAMIC_MIN Ticks (20 Sekunden) kamen. Ein kurzer Effekt (Goldapfel-
// Regeneration) laeuft ohnehin gleich ab, dort waere das Blinken nur Unruhe.
pub(crate) const EFFECTS_BLINK_MODES: [&str; 3] = ["Aus", "An", "Dynamisch"];
pub(crate) const EFFECTS_BLINK_OFF: u8 = 0;
pub(crate) const EFFECTS_BLINK_ON: u8 = 1;
pub(crate) const EFFECTS_BLINK_DYNAMIC: u8 = 2;
pub(crate) const EFFECTS_BLINK_DYNAMIC_MIN: i32 = 400;

// Blinkphase des Timers: Some(true) = rot, Some(false) = weiss, None = blinkt nicht (aus,
// unendlich, mehr als 10 Sekunden uebrig oder dynamisch und zu kurz bekommen). Rot zuerst, je
// halbe Sekunde. Nur nuetzliche Effekte warnen: dass ein schaedlicher oder neutraler bald
// endet, ist keine Gefahr.
pub(crate) fn effect_timer_blink(entry: &EffectEntry, mode: u8) -> Option<bool> {
    let d = entry.remaining;
    let an = world::potion::effect_beneficial(entry.effect) && match mode {
        EFFECTS_BLINK_OFF => false,
        EFFECTS_BLINK_DYNAMIC => entry.received > EFFECTS_BLINK_DYNAMIC_MIN,
        _ => true,
    };
    (an && d != protocol::packets::MOB_EFFECT_INFINITE && d <= EFFECTS_BLINK_TICKS).then(|| (EFFECTS_BLINK_TICKS - d) / EFFECTS_BLINK_HALF % 2 == 0)
}

// Restdauer fuer die detaillierte Anzeige (EFFECTS_TIMER_STYLES), abgerundet wie Vanilla;
// unendlich wie format_effect_duration.
pub(crate) fn format_effect_timer(ticks: i32, style: u8) -> String {
    if ticks == protocol::packets::MOB_EFFECT_INFINITE {
        return format_effect_duration(ticks);
    }
    let s = ticks.max(0) / 20;
    match style {
        0 => s.to_string(),
        1 if s >= 3600 => format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60),
        1 => format!("{}:{:02}", s / 60, s % 60),
        _ => format!("{}.{}", s / 60, s % 60 / 6),
    }
}

// Ein Effekt der detaillierten Anzeige, in Bildschirmpixeln relativ zur linken oberen Ecke
// der Anzeige: Symbol, Name und Timer je mit ihrer linken Kante, dazu die Oberkante der Zeile.
pub(crate) struct EffectDetailPlace {
    pub(crate) entry: EffectEntry,
    pub(crate) x: f32,
    pub(crate) y: f32,
    // Zeilenhoehe: Symbol und Text stehen darin senkrecht mittig.
    pub(crate) h: f32,
    pub(crate) name: String,
    pub(crate) name_x: f32,
    pub(crate) timer: String,
    pub(crate) timer_x: f32,
}

// Wo die Anker der detaillierten Anzeige bei Ausrichtung `align` liegen, wenn sie das
// Rechteck `r` einnimmt: der feste Rand, auf der anderen Achse links bzw. oben.
pub(crate) fn effects_detail_anchor(r: Rect, align: u8) -> (f32, f32) {
    let x = if align == EFFECTS_ALIGN_RIGHT { r.x + r.w } else { r.x };
    let y = if align == EFFECTS_ALIGN_BOTTOM { r.y + r.h } else { r.y };
    (x, y)
}

// ExperienceOrb.getIcon: Bild 0..10 nach dem Wert der Kugel.
pub(crate) fn experience_orb_icon(wert: i32) -> u32 {
    const STUFEN: [i32; 10] = [3, 7, 17, 37, 73, 149, 307, 617, 1237, 2477];
    STUFEN.iter().filter(|&&s| wert >= s).count() as u32
}

// StringUtil.formatTickDuration bei 20 Ticks je Sekunde; unendlich wie
// MobEffectUtil.formatDuration.
pub(crate) fn format_effect_duration(ticks: i32) -> String {
    if ticks == protocol::packets::MOB_EFFECT_INFINITE {
        return protocol::nbt::with_lang_area(protocol::nbt::LangArea::Effects, || protocol::nbt::translate("effect.duration.infinite")).unwrap_or_else(|| "\u{221e}".to_string());
    }
    let s = ticks.max(0) / 20;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

// EffectsInInventory.getEffectName: Name aus der Sprachdatei, Stufen II bis X angehaengt.
pub(crate) fn effect_display_name(entry: &EffectEntry) -> String {
    protocol::nbt::with_lang_area(protocol::nbt::LangArea::Effects, || effect_display_name_in_area(entry))
}

pub(crate) fn effect_display_name_in_area(entry: &EffectEntry) -> String {
    let id = protocol::packets::mob_effect_name(entry.effect);
    let mut name = protocol::nbt::translate(&format!("effect.minecraft.{id}")).unwrap_or_else(|| id.replace('_', " "));
    if (1..=9).contains(&entry.amplifier) {
        let stufe = entry.amplifier + 1;
        name.push(' ');
        name.push_str(&protocol::nbt::translate(&format!("enchantment.level.{stufe}")).unwrap_or_else(|| stufe.to_string()));
    }
    name
}

// Kuerzt `text` so, dass er samt "..." in `max` passt (ComponentRenderUtils.clipText).
pub(crate) fn clip_with_ellipsis(text: &str, max: f32, breite: &dyn Fn(&str) -> f32) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let probe = format!("{out}{c}...");
        if breite(&probe) > max {
            break;
        }
        out.push(c);
    }
    out.push_str("...");
    out
}

// Entity.isStateClimbable: Kletterbloecke und Pulverschnee -- dort zaehlt fuer die Schritte auch
// die senkrechte Strecke.
pub(crate) fn step_climbable(state: u32) -> bool {
    let name = world::block_name(state).unwrap_or("");
    physics::is_climbable(name) || name == "powder_snow"
}

// Klang beim Ausleeren (BucketItem.playEmptyingSound bzw. EntityBucketItem: der Klang des
// Tiers im Eimer).
pub(crate) fn bucket_empty_sound(item: &str) -> &'static str {
    match item {
        "lava_bucket" => "item.bucket.empty_lava",
        "cod_bucket" | "salmon_bucket" | "pufferfish_bucket" | "tropical_fish_bucket" => "item.bucket.empty_fish",
        "axolotl_bucket" => "item.bucket.empty_axolotl",
        "tadpole_bucket" => "item.bucket.empty_tadpole",
        _ => "item.bucket.empty",
    }
}

// Welche Fluessigkeit ein Eimer ausgiesst. Fisch-/Axolotl-Eimer giessen in Vanilla ebenfalls
// Wasser aus (EntityBucketItem erbt placeFluid) -- das Tier setzt nur der Server.
pub(crate) fn bucket_fluid(item: &str) -> Option<&'static str> {
    match item {
        "water_bucket" | "cod_bucket" | "salmon_bucket" | "pufferfish_bucket" | "tropical_fish_bucket" | "axolotl_bucket" | "tadpole_bucket" => {
            Some("water")
        }
        "lava_bucket" => Some("lava"),
        _ => None,
    }
}

// Vanillas Gui.HeartType: dieselbe Fuellung in der Art des Zustands (`art` = "", "poisoned_",
// "withered_" oder "frozen_"). Die Sprites heissen <art>[hardcore_]full/half, nur der Behaelter
// haengt "_hardcore" hinten an. Absorptionsherzen bleiben absorbing -- ausser bei Wither
// (Gui.renderHearts: `heartType == WITHERED ? heartType : ABSORBING`).
pub(crate) fn heart_variant(sprite: &str, art: &str, hardcore: bool) -> String {
    let rest = sprite.trim_start_matches("hud/heart/");
    if rest == "container" {
        return if hardcore { "hud/heart/container_hardcore".to_string() } else { sprite.to_string() };
    }
    let fuellung = rest.rsplit('_').next().unwrap_or(rest);
    let art = if rest.starts_with("absorbing_") && art != "withered_" { "absorbing_" } else { art };
    format!("hud/heart/{art}{}{fuellung}", if hardcore { "hardcore_" } else { "" })
}

// Goldenes Herz Nummer `index` HINTER den normalen: `r = index * 2`, gezeichnet solange
// `r < absorption`, halb genau beim letzten ungeraden Punkt (InGameHud.renderHealthBar).
pub(crate) const fn absorbing_heart_sprite(absorption: i32, index: i32) -> Option<&'static str> {
    let r = index * 2;
    if r >= absorption {
        None
    } else if r + 1 == absorption {
        Some("hud/heart/absorbing_half")
    } else {
        Some("hud/heart/absorbing_full")
    }
}

// Ruestung zeichnet auch die LEEREN Symbole (anders als die Herzen, wo der Behaelter das
// uebernimmt) -- deshalb immer ein Name.
pub(crate) const fn armor_sprite(armor: i32, index: i32) -> &'static str {
    let halb = index * 2 + 1;
    if halb < armor {
        "hud/armor_full"
    } else if halb == armor {
        "hud/armor_half"
    } else {
        "hud/armor_empty"
    }
}

pub(crate) const fn food_sprite(food: i32, index: i32) -> Option<&'static str> {
    let halb = index * 2 + 1;
    if halb < food {
        Some("hud/food_full")
    } else if halb == food {
        Some("hud/food_half")
    } else {
        None
    }
}

// Vanilla: ceil(air * 10 / maxAir) volle Blasen -- bei 300 Ticks also genau 10, und die
// letzte Blase verschwindet erst, wenn die Luft wirklich auf 0 steht.
pub(crate) fn air_bubbles(air_ticks: i32) -> i32 {
    ((air_ticks.max(0) as f32 * 10.0) / MAX_AIR_TICKS as f32).ceil() as i32
}

// Das Spielfenster: exklusives Vollbild im groessten Modus des Hauptmonitors. Legt beim Start
// der Ladebildschirm an (s. loading_screen), sonst App::resumed.
// Die Regel hinter App::latched_width: je Schluessel die groesste je gesehene Breite.
pub(crate) fn latch_width(widths: &mut std::collections::HashMap<&'static str, f32>, key: &'static str, width: f32) -> f32 {
    let latched = widths.entry(key).or_insert(width);
    *latched = latched.max(width);
    *latched
}
