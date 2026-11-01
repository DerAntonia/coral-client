// Item-Angaben fuer Anzeige und Tooltips: Haltbarkeit, Namen, Attribute, Karten.

use crate::*;

// TridentItem.onStoppedUsing: unter 10 Ticks Benutzung passiert beim Loslassen nichts.
pub(crate) const TRIDENT_MIN_USE_TICKS: u32 = 10;
// Haltbarkeit des Dreizacks, wenn der Stack keine eigene nennt (items.json: max_damage 250).
pub(crate) const TRIDENT_MAX_DAMAGE: i32 = 250;

// No Tool Break: nur Diamant- und Netherit-Werkzeuge, mit der Standard-Haltbarkeit ihrer Stufe.
pub(crate) fn protected_tool_durability(name: &str) -> Option<i32> {
    let (stufe, art) = name.split_once('_')?;
    if !matches!(art, "pickaxe" | "axe" | "shovel" | "hoe" | "sword") {
        return None;
    }
    match stufe {
        "diamond" => Some(DIAMOND_TOOL_DURABILITY),
        "netherite" => Some(NETHERITE_TOOL_DURABILITY),
        _ => None,
    }
}

// Vanillas Standard-Haltbarkeit (max_damage-Komponente) je Item -- der Server schickt
// max_damage nur mit, wenn er vom Standard abweicht. None = das Item hat keine Haltbarkeit.
// Werkzeuge nach Material (ToolMaterial), Ruestung nach Material mal Teil (ArmorMaterial:
// Helm 11, Brust 16, Hose 15, Stiefel 13).
pub(crate) fn default_max_damage(name: &str) -> Option<i32> {
    let fest = match name {
        "bow" => 384,
        "crossbow" => 465,
        "trident" => 250,
        "shield" => 336,
        "elytra" => 432,
        "mace" => 500,
        "fishing_rod" | "flint_and_steel" | "brush" | "wolf_armor" => 64,
        "shears" => 238,
        "carrot_on_a_stick" => 25,
        "warped_fungus_on_a_stick" => 100,
        "turtle_helmet" => 275,
        _ => 0,
    };
    if fest > 0 {
        return Some(fest);
    }
    let (stufe, art) = name.split_once('_')?;
    let werkzeug = match stufe {
        "wooden" => Some(59),
        "stone" => Some(131),
        "copper" => Some(190),
        "iron" => Some(250),
        "golden" => Some(32),
        "diamond" => Some(1561),
        "netherite" => Some(2031),
        _ => None,
    };
    if matches!(art, "pickaxe" | "axe" | "shovel" | "hoe" | "sword" | "spear") {
        return werkzeug;
    }
    let teil = match art {
        "helmet" => 11,
        "chestplate" => 16,
        "leggings" => 15,
        "boots" => 13,
        _ => return None,
    };
    let material = match stufe {
        "leather" => 5,
        "copper" => 11,
        "chainmail" | "iron" => 15,
        "golden" => 7,
        "diamond" => 33,
        "netherite" => 37,
        _ => return None,
    };
    Some(teil * material)
}

// Vanillas Haltbarkeitsbalken (ItemStack.getItemBarStep/-Color): Breite in 16tel-Slot-Pixeln
// (0..13) und Farbe von Gruen ueber Gelb nach Rot. None, solange das Item unbeschaedigt ist.
// Braucht ein Item einen Tooltip, um erkannt zu werden? Ja, wenn etwas daran haengt, das
// das Symbol nicht zeigt: Verzauberungen (auch gespeicherte), ein eigener Name oder
// Server-Itemname, Lore, ein Trank, ein Ruestungsbesatz, Bannermuster, ein Kopf mit Profil
// oder ein anderes Modell. Ein gewoehnlicher Schneeball, Stein oder ein Diamantschwert ohne
// Verzauberung braucht keinen -- Haltbarkeit zeigt der Balken.
pub(crate) fn item_needs_tooltip(stack: &protocol::item::ItemStack) -> bool {
    !stack.enchantments.is_empty()
        || !stack.stored_enchantments.is_empty()
        || stack.custom_name.is_some()
        || stack.item_name_spans.is_some()
        || !stack.lore.is_empty()
        || stack.potion.is_some()
        || stack.trim.is_some()
        || !stack.banner_patterns.is_empty()
        || stack.profile.is_some()
        || stack.item_model.is_some()
        || world::bundle::is_bundle(stack)
        || stack.written_book.is_some()
}

// Zeigt der Stapel beim Wechsel auf ihn seinen Namen ueber der Hotbar? Nur mit eigenem Namen
// (umbenannt oder vom Server benannt) oder Lore -- Verzauberungen, Traenke oder Trims allein
// reichen nicht, die zeigt der Tooltip im Inventar.
pub(crate) const fn item_shows_held_name(stack: &protocol::item::ItemStack) -> bool {
    stack.custom_name.is_some() || stack.item_name_spans.is_some() || !stack.lore.is_empty()
}

// Wert eines Attributs, das ein Item anlegt, ausgehend von `basis` (dem Grundwert des
// Spielers): erst alle add_value, dann add_multiplied_base auf diese Summe, dann je
// add_multiplied_total mal (1 + Betrag) -- wie AttributeInstance.calculateValue. None, wenn
// das Item das Attribut in diesen Slots gar nicht anfasst.
pub(crate) fn item_attribute_value(mods: &[protocol::item::ItemAttributeModifier], attribute: i32, basis: f64, slot: &dyn Fn(i32) -> bool) -> Option<f64> {
    let passend: Vec<_> = mods.iter().filter(|m| m.attribute == attribute && slot(m.slot)).collect();
    if passend.is_empty() {
        return None;
    }
    let summe = basis + passend.iter().filter(|m| m.operation == 0).map(|m| m.amount).sum::<f64>();
    let mut wert = summe + passend.iter().filter(|m| m.operation == 1).map(|m| summe * m.amount).sum::<f64>();
    for m in passend.iter().filter(|m| m.operation == 2) {
        wert *= 1.0 + m.amount;
    }
    Some(wert)
}

// Zahl eines Addon-Reglers: ohne Nullen hinten, deutsches Komma ("2,5", "10").
pub(crate) fn addon_number_text(v: f64) -> String {
    let t = format!("{:.3}", (v * 1000.0).round() / 1000.0);
    t.trim_end_matches('0').trim_end_matches('.').replace('.', ",")
}

// Lautstaerke fuers Feld: bis zu zwei Nachkommastellen mit deutschem Komma, ohne Nullen
// hinten ("12,5 %", "100 %").
pub(crate) fn volume_text(v: f32) -> String {
    let t = format!("{:.2}", (v * 100.0).round() / 100.0);
    format!("{} %", t.trim_end_matches('0').trim_end_matches('.').replace('.', ","))
}

// Getippte oder gespeicherte Lautstaerke (Komma oder Punkt), unveraendert.
pub(crate) fn parse_volume(text: &str) -> Option<f32> {
    let v: f32 = text.trim().replace(',', ".").parse().ok()?;
    v.is_finite().then_some(v)
}

// Zahl mit hoechstens einer Nachkommastelle und deutschem Komma, ohne ",0".
pub(crate) fn decimal_de(v: f64) -> String {
    let gerundet = (v * 10.0).round() / 10.0;
    if gerundet.fract() == 0.0 {
        format!("{}", gerundet as i64)
    } else {
        format!("{gerundet:.1}").replace('.', ",")
    }
}

// So viele Ticks nach dem lokalen Fertigessen wartet der naechste Bissen (s. tick_item_use).
pub(crate) const EAT_NEXT_USE_DELAY: u32 = 1;

// Modul Delay: Grenzen und Schrittweite des Reglers fuer die Verzoegerung je Richtung (s. net::lag).
pub(crate) const LAG_DELAY_MAX_MS: u32 = 800;
pub(crate) const LAG_DELAY_STEP_MS: u32 = 1;

// Fehlen nach `benutzt` von `dauer` Ticks hoechstens `grenze_ms` Millisekunden? Ein Tick sind
// 50 ms; schon fertig zaehlt als "nichts fehlt".
pub(crate) const fn eat_rest_within(dauer: u32, benutzt: u32, grenze_ms: u32) -> bool {
    dauer.saturating_sub(benutzt) * 50 <= grenze_ms
}

// Bogen (BowItem.releaseUsing / getPowerForTime): unter Kraft 0.1 -- weniger als 3 Ticks
// gespannt -- fliegt kein Pfeil, ab 20 Ticks ist er voll (kritisch). Ohne Puffer: bei
// stabilem Ping kommen USE und RELEASE beim Server im selben Abstand an, in dem sie gingen.
pub(crate) const BOW_MIN_SHOT_TICKS: u32 = 3;
pub(crate) const BOW_FULL_SHOT_TICKS: u32 = 20;

// Usage-Optimizer am Bogen: Muss die Benutzung nach `benutzt` Ticks noch weiterlaufen? Bis
// ein Pfeil ueberhaupt fliegt immer; bis zur vollen Kraft, wenn hoechstens `grenze_ms` fehlen.
pub(crate) const fn bow_hold_through(benutzt: u32, grenze_ms: u32) -> bool {
    benutzt < BOW_MIN_SHOT_TICKS || (benutzt < BOW_FULL_SHOT_TICKS && grenze_ms > 0 && eat_rest_within(BOW_FULL_SHOT_TICKS, benutzt, grenze_ms))
}

// Hoechsthaltbarkeit eines Stapels, None ohne Haltbarkeit (oder unzerbrechlich, dann zeigt
// Vanilla keinen Balken): die wirksame max_damage-Component, ohne Standards die Tabelle.
pub(crate) fn max_damage_of(stack: &protocol::item::ItemStack, name: &str) -> Option<i32> {
    if world::item_components::loaded() {
        return world::item_behavior::damageable(stack).then(|| world::item_components::Components::of(stack).max_damage());
    }
    stack.max_damage.or_else(|| default_max_damage(name))
}

// Verbleibende Haltbarkeit 0..1, None fuer Items ohne Haltbarkeit.
pub(crate) fn durability_rest(stack: &protocol::item::ItemStack, name: &str) -> Option<f32> {
    let max = max_damage_of(stack, name)?;
    (max > 0).then(|| ((max - stack.damage.unwrap_or(0)) as f32 / max as f32).clamp(0.0, 1.0))
}

// Verbleibende Haltbarkeitspunkte, None fuer Items ohne Haltbarkeit.
pub(crate) fn durability_points(stack: &protocol::item::ItemStack, name: &str) -> Option<i32> {
    let max = max_damage_of(stack, name)?;
    (max > 0).then(|| (max - stack.damage.unwrap_or(0)).max(0))
}

pub(crate) fn item_bar(stack: &protocol::item::ItemStack, name: &str) -> Option<(f32, [f32; 3])> {
    let damage = stack.damage.unwrap_or(0);
    let max = max_damage_of(stack, name)?;
    if damage <= 0 || max <= 0 {
        return None;
    }
    let schritt = (13.0 - damage as f32 * 13.0 / max as f32).round().clamp(0.0, 13.0);
    let rest = ((max - damage) as f32 / max as f32).max(0.0);
    // MathHelper.hsvToRgb(f / 3, 1, 1): Ton 0 (rot) bis 1/3 (gruen).
    let ton = rest / 3.0 * 6.0;
    let x = 1.0 - (ton % 2.0 - 1.0).abs();
    let farbe = if ton < 1.0 { [1.0, x, 0.0] } else { [x, 1.0, 0.0] };
    Some((schritt, farbe))
}

// Fuer die Benachrichtigung: ein umbenanntes Werkzeug unter seinem Namen, sonst lesbar.
pub(crate) fn tool_display_name(stack: &protocol::item::ItemStack, name: &str) -> String {
    if let Some(eigen) = &stack.custom_name {
        return eigen.clone();
    }
    let (stufe, art) = name.split_once('_').unwrap_or(("", name));
    let stufe = match stufe {
        "diamond" => "Diamant",
        "netherite" => "Netherit",
        s => s,
    };
    let art = match art {
        "pickaxe" => "spitzhacke",
        "axe" => "axt",
        "shovel" => "schaufel",
        "hoe" => "hacke",
        "sword" => "schwert",
        a => a,
    };
    format!("{stufe}{art}")
}

// ItemStack.willBreakNextUse: beschaedigbar und nur noch ein Punkt Haltbarkeit uebrig. Nur fuer
// den Dreizack gebraucht -- ohne Standards ist dessen Haltbarkeit deshalb der Rueckfall.
pub(crate) fn will_break_next_use(stack: &protocol::item::ItemStack) -> bool {
    if world::item_components::loaded() {
        return world::item_behavior::next_damage_will_break(stack);
    }
    stack.damage.unwrap_or(0) >= stack.max_damage.unwrap_or(TRIDENT_MAX_DAMAGE) - 1
}

// CrossbowPullProperty: Benutzungsdauer durch Ladezeit. Bei Ladezeit 0 (Schnellladen V per
// Befehl) teilt Vanilla durch null und landet bei +Unendlich -- sofort voll gespannt.
// Consumable.shouldEmitParticlesAndSounds: Kruemel erst nach gut einem Fuenftel der Essdauer,
// dann alle vier Ticks. `ticks` zaehlt den laufenden Tick mit (1 im ersten Tick); die Restzeit
// darf negativ werden -- bei anderen Spielern isst es weiter, bis der Server Schluss sagt.
pub(crate) fn eating_crumbs_due(dauer: u32, ticks: u32) -> bool {
    let benutzt = i64::from(ticks) - 1;
    let rest = i64::from(dauer) - benutzt;
    benutzt > (dauer as f32 * 0.21875) as i64 && rest.rem_euclid(4) == 0
}

#[cfg(test)]
pub(crate) mod kruemel_tests {
    // Brot (32 Ticks): die ersten sieben Ticks nichts, dann alle vier Ticks -- und nach Ablauf
    // der Dauer weiter im selben Takt.
    #[test]
    fn kruemel_nach_einem_fuenftel_alle_vier_ticks() {
        let ticks: Vec<u32> = (1..=45).filter(|&t| super::eating_crumbs_due(32, t)).collect();
        assert_eq!(ticks, vec![9, 13, 17, 21, 25, 29, 33, 37, 41, 45]);
    }
}

// Welches Item ein Item-Partikel zeigt: das Modell (item_model), wenn es ein Vanilla-Item
// ist -- ein Packmodell hat keinen Platz im Partikel-Atlas, dann das Item selbst.
pub(crate) fn particle_item_name(h: &world::HeldItem) -> Option<&'static str> {
    let id = if h.model_id < world::entity::CUSTOM_MODEL_BASE { h.model_id } else { h.item_id };
    world::item_registry::item_name(id)
}

pub(crate) fn crossbow_pull_progress(use_ticks: f32, pull_ticks: f32) -> f32 {
    if pull_ticks <= 0.0 {
        f32::INFINITY
    } else {
        use_ticks / pull_ticks
    }
}

// Der Tempo-Faktor aus use_effects (f32) fuer die f64-Physik: als die Dezimalzahl, die er
// darstellt (0.2, nicht 0.20000000298...) -- so rechnete die Bewegung schon vorher, und so
// bleibt sie Bit fuer Bit dieselbe.
pub(crate) fn use_speed_f64(m: f32) -> f64 {
    m.to_string().parse().unwrap_or(f64::from(m))
}

// Ein Kartenbild als Viereck 0..128 in x/y (MapRenderer: y nach unten = v nach unten), unter
// `m` in die Welt gesetzt.
pub(crate) fn push_map_quad(m: world::entity_model::Affine, [u0, v0, u1, v1]: [f32; 4], hell: f32, out: &mut Vec<world::BlockVertex>) {
    let ecke = |x: f32, y: f32, u: f32, v: f32| world::BlockVertex { position: m.apply([x, y, -0.01]), color: [hell; 3], uv: [u, v] };
    let (a, b, c, d) = (ecke(0.0, 128.0, u0, v1), ecke(128.0, 128.0, u1, v1), ecke(128.0, 0.0, u1, v0), ecke(0.0, 0.0, u0, v0));
    out.extend([a, b, c, a, c, d]);
}

// ItemInHandRenderer.calculateMapTilt: je weiter man nach unten schaut, desto flacher liegt die
// Karte (0 beim Geradeausblick bis 1 ab etwa 50 Grad nach unten), weich ueber einen Kosinus.
pub(crate) fn map_tilt(pitch_deg: f32) -> f32 {
    let t = (1.0 - pitch_deg / 45.0 + 0.1).clamp(0.0, 1.0);
    -(t * std::f32::consts::PI).cos() * 0.5 + 0.5
}

// Ein belegter Kartenplatz im Entity-Atlas (s. App::ensure_map_textures).
#[derive(Clone, Copy)]
pub(crate) struct MapSlot {
    pub(crate) id: i32,
    pub(crate) im_rahmen: bool,
    pub(crate) revision: u32,
    pub(crate) zuletzt: u64,
}

// Breite des Buendel-Tooltips in GUI-Pixeln (ClientBundleTooltip).
pub(crate) const BUNDLE_PANEL_W: f32 = 96.0;

// Bis zu so vielen Zweigen werden alle Modelle einer Definition in der eigenen Hand vorgeladen
// (s. prepare_held_items).
pub(crate) const HAND_MODEL_PRELOAD_MAX: usize = 8;

// Welche Item-Definition (items/<id>.json) einen Stapel zeichnet: die item_model-Component
// ("minecraft:mace" -> "mace", "uhc:storm_wall" bleibt), sonst der Itemname.
pub(crate) fn model_definition_id(stack: &protocol::item::ItemStack, name: &str) -> String {
    match stack.item_model.as_deref() {
        Some(m) => m.strip_prefix("minecraft:").unwrap_or(m).to_string(),
        None => name.to_string(),
    }
}

// Welches Icon ein Stack bekommt. Meist der Itemname; die geladene Armbrust zeigt wie in
// Vanilla (items/crossbow.json, select auf charge_type) den Pfeil bzw. die Rakete.
pub(crate) fn item_icon_name<'a>(name: &'a str, stack: &protocol::item::ItemStack) -> &'a str {
    if name == "crossbow" && stack.is_charged() {
        let rakete = world::item_registry::item_id_by_name("firework_rocket");
        return if stack.charged_projectiles.iter().any(|id| Some(*id) == rakete) { "crossbow_firework" } else { "crossbow_arrow" };
    }
    name
}
