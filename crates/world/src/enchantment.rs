// Verzauberungen: was der CLIENT davon selbst wissen muss.
//
// Seit 1.21 sind Verzauberungen datengetrieben. Welche ID "Schaerfe" ist und was "Schnell-
// laden" genau tut, steht nicht im Code, sondern in der Verzauberungs-Registry, die der Server
// in der Konfigurationsphase schickt (s. protocol::packets::read_enchantments). Diese Datei
// liest daraus genau die Effekte, die der Client ausfuehrt oder anzeigt:
//
//   crossbow_charge_time          Schnellladen   -- wie lange die Armbrust gespannt wird
//   trident_spin_attack_strength  Sog            -- der Schub beim Loslassen des Dreizacks
//   prevent_armor_change          Fluch der Bindung -- Ruestung laesst sich nicht abnehmen
//
// Alles, was ueber ATTRIBUTE wirkt (Effizienz, Wasseraffinitaet, Wasserlaeufer, Schleichender
// Schritt, Seelenlaeufer), braucht hier nichts: der Server schickt das fertige Attribut mit
// (s. protocol::packets::read_attributes). Der Rest (Schaden, Schutz, Haltbarkeit, Beute ...)
// passiert allein auf dem Server.
//
// Dazu die Anzeige: Glint (has_glint) und die Tooltip-Zeilen (tooltip_lines).

use protocol::item::{Enchantment, ItemStack};
use protocol::nbt::{NbtValue, TextSpan};

// Items, deren enchantment_glint_override im Default schon `true` ist (items.json aus dem
// 1.21.11-Datengenerator). Sie glitzern auch ohne Verzauberung.
const DEFAULT_GLINT: &[&str] =
    &["debug_stick", "enchanted_book", "enchanted_golden_apple", "end_crystal", "experience_bottle", "nether_star", "written_book"];

// Tag #minecraft:curse -- Flueche stehen im Tooltip rot statt grau. Tags schickt der Server
// zwar auch, dieser Client liest sie aber nicht; die Vanilla-Liste reicht dafuer.
const CURSES: &[&str] = &["minecraft:binding_curse", "minecraft:vanishing_curse"];

// Tag #minecraft:tooltip_order (1.21.11-Jar): in DIESER Reihenfolge erscheinen die Zeilen,
// alles andere danach in Item-Reihenfolge (ItemEnchantmentsComponent.appendTooltip).
const TOOLTIP_ORDER: &[&str] = &[
    "minecraft:binding_curse",
    "minecraft:vanishing_curse",
    "minecraft:riptide",
    "minecraft:channeling",
    "minecraft:wind_burst",
    "minecraft:frost_walker",
    "minecraft:lunge",
    "minecraft:sharpness",
    "minecraft:smite",
    "minecraft:bane_of_arthropods",
    "minecraft:impaling",
    "minecraft:power",
    "minecraft:density",
    "minecraft:breach",
    "minecraft:piercing",
    "minecraft:sweeping_edge",
    "minecraft:multishot",
    "minecraft:fire_aspect",
    "minecraft:flame",
    "minecraft:knockback",
    "minecraft:punch",
    "minecraft:protection",
    "minecraft:blast_protection",
    "minecraft:fire_protection",
    "minecraft:projectile_protection",
    "minecraft:feather_falling",
    "minecraft:fortune",
    "minecraft:looting",
    "minecraft:silk_touch",
    "minecraft:luck_of_the_sea",
    "minecraft:efficiency",
    "minecraft:quick_charge",
    "minecraft:lure",
    "minecraft:respiration",
    "minecraft:aqua_affinity",
    "minecraft:soul_speed",
    "minecraft:swift_sneak",
    "minecraft:depth_strider",
    "minecraft:thorns",
    "minecraft:loyalty",
    "minecraft:unbreaking",
    "minecraft:infinity",
    "minecraft:mending",
];

// Formatting.GRAY / Formatting.RED.
const GRAY: u32 = 0xAAAAAA;
const RED: u32 = 0xFF5555;

// CrossbowItem.DEFAULT_PULL_TIME (Sekunden).
const CROSSBOW_BASE_PULL_SECONDS: f32 = 1.25;

// ItemStack.hasGlint(): ein GESETZTER Override gewinnt; sonst der Default-Override des
// Items (sofern der Server ihn nicht entfernt hat); sonst "hat Verzauberungen". Gespeicherte
// Verzauberungen (Buch) zaehlen dabei NICHT -- das Buch glitzert ueber seinen Default.
pub fn has_glint(stack: &ItemStack) -> bool {
    if let Some(glint) = stack.glint_override {
        return glint;
    }
    let default_override = !stack.removed_components.contains(&protocol::item::COMPONENT_ENCHANTMENT_GLINT_OVERRIDE)
        && crate::item_registry::item_name(stack.item_id).is_some_and(|name| DEFAULT_GLINT.contains(&name));
    default_override || !stack.enchantments.is_empty()
}

// LevelBasedValue: eine Zahl, die von der Verzauberungsstufe abhaengt.
#[derive(Clone, Debug, PartialEq)]
enum LevelValue {
    Constant(f32),
    Linear { base: f32, per_level_above_first: f32 },
    Clamped { value: Box<LevelValue>, min: f32, max: f32 },
    Fraction { numerator: Box<LevelValue>, denominator: Box<LevelValue> },
    LevelsSquared { added: f32 },
    Lookup { values: Vec<f32>, fallback: Box<LevelValue> },
}

impl LevelValue {
    fn parse(v: &NbtValue) -> Option<Self> {
        if let Some(n) = v.as_f32() {
            return Some(LevelValue::Constant(n));
        }
        let f = |key: &str| v.get(key).and_then(NbtValue::as_f32);
        let sub = |key: &str| v.get(key).and_then(LevelValue::parse).map(Box::new);
        Some(match v.get("type")?.as_str()?.trim_start_matches("minecraft:") {
            "constant" => LevelValue::Constant(f("value")?),
            "linear" => LevelValue::Linear { base: f("base")?, per_level_above_first: f("per_level_above_first")? },
            "clamped" => LevelValue::Clamped { value: sub("value")?, min: f("min")?, max: f("max")? },
            "fraction" => LevelValue::Fraction { numerator: sub("numerator")?, denominator: sub("denominator")? },
            "levels_squared" => LevelValue::LevelsSquared { added: f("added")? },
            "lookup" => LevelValue::Lookup {
                values: match v.get("values")? {
                    NbtValue::List(l) => l.iter().filter_map(NbtValue::as_f32).collect(),
                    NbtValue::IntArray(l) => l.iter().map(|&i| i as f32).collect(),
                    _ => return None,
                },
                fallback: sub("fallback")?,
            },
            _ => return None,
        })
    }

    fn get(&self, level: i32) -> f32 {
        match self {
            LevelValue::Constant(v) => *v,
            LevelValue::Linear { base, per_level_above_first } => base + per_level_above_first * (level - 1) as f32,
            LevelValue::Clamped { value, min, max } => value.get(level).clamp(*min, *max),
            LevelValue::Fraction { numerator, denominator } => {
                let d = denominator.get(level);
                if d == 0.0 {
                    0.0
                } else {
                    numerator.get(level) / d
                }
            }
            LevelValue::LevelsSquared { added } => (level * level) as f32 + added,
            LevelValue::Lookup { values, fallback } => {
                usize::try_from(level - 1).ok().and_then(|i| values.get(i).copied()).unwrap_or_else(|| fallback.get(level))
            }
        }
    }
}

// EnchantmentValueEffect: wie ein Wert veraendert wird. remove_binomial ist zufallsbasiert und
// kommt bei den hier gebrauchten Effekten in Vanilla nicht vor -- wird ignoriert.
#[derive(Clone, Debug, PartialEq)]
enum ValueEffect {
    Add(LevelValue),
    Multiply(LevelValue),
    Set(LevelValue),
    AllOf(Vec<ValueEffect>),
}

impl ValueEffect {
    fn parse(v: &NbtValue) -> Option<Self> {
        let lv = |key: &str| v.get(key).and_then(LevelValue::parse);
        Some(match v.get("type")?.as_str()?.trim_start_matches("minecraft:") {
            "add" => ValueEffect::Add(lv("value")?),
            "multiply" => ValueEffect::Multiply(lv("factor")?),
            "set" => ValueEffect::Set(lv("value")?),
            "all_of" => match v.get("effects")? {
                NbtValue::List(l) => ValueEffect::AllOf(l.iter().filter_map(ValueEffect::parse).collect()),
                _ => return None,
            },
            _ => return None,
        })
    }

    fn apply(&self, level: i32, input: f32) -> f32 {
        match self {
            ValueEffect::Add(v) => input + v.get(level),
            ValueEffect::Multiply(v) => input * v.get(level),
            ValueEffect::Set(v) => v.get(level),
            ValueEffect::AllOf(effects) => effects.iter().fold(input, |acc, e| e.apply(level, acc)),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct EnchantmentDef {
    // Registry-Name, z.B. "minecraft:sharpness".
    pub name: String,
    // Text-Component der Beschreibung (meist {"translate": "enchantment.minecraft.<name>"}).
    description: Option<NbtValue>,
    max_level: i32,
    crossbow_charge_time: Option<ValueEffect>,
    trident_spin_attack_strength: Option<ValueEffect>,
    prevent_armor_change: bool,
    // minecraft:damage ohne Bedingung -- Bonus auf jeden Treffer (Schaerfe). Bedingte wie
    // Bann oder Nemesis gelten nur gegen bestimmte Mobs und zaehlen hier nicht.
    damage: Vec<ValueEffect>,
    // minecraft:damage_protection, nach der Schadensart, gegen die es schuetzt.
    protection: Vec<(ProtectionKind, ValueEffect)>,
}

// Wogegen ein Schutz-Effekt wirkt -- erkannt an den verlangten Schadens-Tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionKind {
    General,
    Explosion,
    Fire,
    Projectile,
    Fall,
    // Eine Bedingung, die keiner dieser Arten entspricht (eigene Verzauberungen).
    Other,
}

impl ProtectionKind {
    // Aus der requirements-Bedingung eines Effekts: welche Tags verlangt werden (expected
    // true). Keiner -> wirkt gegen alles (bis auf das, was Unverwundbarkeit durchbricht).
    fn of(requirements: Option<&NbtValue>) -> Self {
        let mut verlangt = Vec::new();
        if let Some(r) = requirements {
            collect_required_tags(r, &mut verlangt);
        }
        match verlangt.as_slice() {
            [] => ProtectionKind::General,
            [t] => match t.trim_start_matches('#').trim_start_matches("minecraft:") {
                "is_explosion" => ProtectionKind::Explosion,
                "is_fire" => ProtectionKind::Fire,
                "is_projectile" => ProtectionKind::Projectile,
                "is_fall" => ProtectionKind::Fall,
                _ => ProtectionKind::Other,
            },
            _ => ProtectionKind::Other,
        }
    }
}

// Sammelt aus einer Bedingung (auch verschachtelt in all_of/terms) alle Tag-Pruefungen mit
// expected = true. Eine Bedingung, die keine Schadensquelle prueft, macht den Effekt
// unbestimmbar -- dann landet ein Platzhalter in der Liste.
fn collect_required_tags(v: &NbtValue, out: &mut Vec<String>) {
    let art = v.get("condition").and_then(NbtValue::as_str).unwrap_or("").trim_start_matches("minecraft:");
    match art {
        "all_of" => {
            if let Some(NbtValue::List(l)) = v.get("terms") {
                for t in l {
                    collect_required_tags(t, out);
                }
            }
        }
        "damage_source_properties" => {
            if let Some(NbtValue::List(tags)) = v.get("predicate").and_then(|p| p.get("tags")) {
                for t in tags {
                    let erwartet = t.get("expected").and_then(NbtValue::as_i32).unwrap_or(0) != 0;
                    if let (true, Some(id)) = (erwartet, t.get("id").and_then(NbtValue::as_str)) {
                        out.push(id.to_string());
                    }
                }
            }
        }
        _ => out.push(format!("?{art}")),
    }
}

// Die Liste eines bedingten Effekts ({effect, requirements?}) mit Bedingung.
fn conditional_effects(v: Option<&NbtValue>) -> Vec<(Option<&NbtValue>, ValueEffect)> {
    let Some(NbtValue::List(l)) = v else { return Vec::new() };
    l.iter().filter_map(|e| Some((e.get("requirements"), ValueEffect::parse(e.get("effect")?)?))).collect()
}

impl EnchantmentDef {
    pub fn parse(name: String, data: Option<&NbtValue>) -> Self {
        let effects = data.and_then(|d| d.get("effects"));
        let effect = |key: &str| effects.and_then(|e| e.get(key));
        EnchantmentDef {
            description: data.and_then(|d| d.get("description")).cloned(),
            max_level: data.and_then(|d| d.get("max_level")).and_then(NbtValue::as_i32).unwrap_or(1),
            crossbow_charge_time: effect("minecraft:crossbow_charge_time").and_then(ValueEffect::parse),
            trident_spin_attack_strength: effect("minecraft:trident_spin_attack_strength").and_then(ValueEffect::parse),
            prevent_armor_change: effect("minecraft:prevent_armor_change").is_some(),
            damage: conditional_effects(effect("minecraft:damage")).into_iter().filter(|(r, _)| r.is_none()).map(|(_, e)| e).collect(),
            protection: conditional_effects(effect("minecraft:damage_protection")).into_iter().map(|(r, e)| (ProtectionKind::of(r), e)).collect(),
            name,
        }
    }
}

// Die Registry, Index = Verzauberungs-ID. Leer bis zur ersten Konfiguration -- dann hat kein
// Item eine bekannte Verzauberung und alles verhaelt sich wie unverzaubert.
#[derive(Clone, Debug, Default)]
pub struct Enchantments {
    defs: Vec<EnchantmentDef>,
}

impl Enchantments {
    pub fn new(defs: Vec<EnchantmentDef>) -> Self {
        Self { defs }
    }

    pub fn from_registry(entries: Vec<(String, Option<NbtValue>)>) -> Self {
        Self::new(entries.into_iter().map(|(name, data)| EnchantmentDef::parse(name, data.as_ref())).collect())
    }

    fn def(&self, e: &Enchantment) -> Option<&EnchantmentDef> {
        self.defs.get(e.id as usize)
    }

    // EnchantmentHelper.forEachEnchantment: nur die WIRKENDEN Verzauberungen (nicht die
    // gespeicherten eines Buchs), mit Stufe > 0.
    fn active<'a>(&'a self, stack: &'a ItemStack) -> impl Iterator<Item = (&'a EnchantmentDef, i32)> + 'a {
        stack.enchantments.iter().filter(|e| e.level > 0).filter_map(|e| Some((self.def(e)?, e.level)))
    }

    // CrossbowItem.getPullTime: Grundzeit 1,25 s durch Schnellladen verkuerzt, nie negativ,
    // auf ganze Ticks ABGERUNDET.
    pub fn crossbow_pull_ticks(&self, stack: &ItemStack) -> u32 {
        let seconds = self
            .active(stack)
            .filter_map(|(d, level)| d.crossbow_charge_time.as_ref().map(|eff| (eff, level)))
            .fold(CROSSBOW_BASE_PULL_SECONDS, |acc, (eff, level)| eff.apply(level, acc))
            .max(0.0);
        (seconds * 20.0).floor() as u32
    }

    // EnchantmentHelper.getTridentSpinAttackStrength: Grundwert 0 -- groesser 0 heisst Sog.
    pub fn trident_spin_strength(&self, stack: &ItemStack) -> f32 {
        self.active(stack)
            .filter_map(|(d, level)| d.trident_spin_attack_strength.as_ref().map(|eff| (eff, level)))
            .fold(0.0, |acc, (eff, level)| eff.apply(level, acc))
    }

    // Traegt der Stack diese Verzauberung (voller Name, z.B. "minecraft:mending") wirksam?
    pub fn has(&self, stack: &ItemStack, name: &str) -> bool {
        self.active(stack).any(|(d, _)| d.name == name)
    }

    // Schadensbonus der Verzauberungen auf jeden Treffer (Schaerfe), ausgehend vom
    // Grundschaden `base` wie EnchantmentHelper.modifyDamage -- zurueck kommt nur der Zuschlag.
    pub fn damage_bonus(&self, stack: &ItemStack, base: f32) -> f32 {
        let mit = self.active(stack).flat_map(|(d, level)| d.damage.iter().map(move |e| (e, level))).fold(base, |acc, (e, level)| e.apply(level, acc));
        mit - base
    }

    // Schutzpunkte dieses Stapels gegen eine Schadensart (EnchantmentHelper.
    // getDamageProtection fuer ein einzelnes Teil). General zaehlt bei jeder Art mit.
    pub fn protection(&self, stack: &ItemStack, kind: ProtectionKind) -> f32 {
        self.active(stack)
            .flat_map(|(d, level)| d.protection.iter().map(move |(k, e)| (*k, e, level)))
            .filter(|(k, _, _)| *k == kind)
            .fold(0.0, |acc, (_, e, level)| e.apply(level, acc))
    }

    // EnchantmentHelper.hasAnyEnchantmentsWith(stack, PREVENT_ARMOR_CHANGE).
    pub fn prevents_armor_change(&self, stack: &ItemStack) -> bool {
        self.active(stack).any(|(d, _)| d.prevent_armor_change)
    }

    // Registry-Name nach ID ("minecraft:breach").
    pub fn name(&self, id: u32) -> Option<&str> {
        self.defs.get(id as usize).map(|d| d.name.as_str())
    }

    // Name und Stufe einer Verzauberung nach Registry-ID (Hinweis am Zaubertisch).
    pub fn name_by_id(&self, id: i32, level: i32) -> Option<Vec<TextSpan>> {
        let d = self.defs.get(usize::try_from(id).ok()?)?;
        Some(name_line(d, level))
    }

    // Die Tooltip-Zeilen wie ItemEnchantmentsComponent.appendTooltip + Enchantment.getName:
    // erst in Tooltip-Reihenfolge, dann der Rest; Flueche rot, sonst grau (nur wo die
    // Beschreibung keine eigene Farbe hat); die Stufe als roemische Zahl, ausser bei einer
    // Verzauberung, die ohnehin nur Stufe I kennt. Ein Buch zeigt seine gespeicherten.
    pub fn tooltip_lines(&self, stack: &ItemStack) -> Vec<Vec<TextSpan>> {
        let liste = if stack.enchantments.is_empty() { &stack.stored_enchantments } else { &stack.enchantments };
        let bekannt: Vec<(&EnchantmentDef, i32)> =
            liste.iter().filter(|e| e.level > 0).filter_map(|e| Some((self.def(e)?, e.level))).collect();
        let rang = |d: &EnchantmentDef| TOOLTIP_ORDER.iter().position(|n| *n == d.name).unwrap_or(usize::MAX);
        let mut sortiert = bekannt.clone();
        // Stabil: Unsortierte (rang MAX) behalten ihre Item-Reihenfolge.
        sortiert.sort_by_key(|(d, _)| rang(d));
        sortiert.into_iter().map(|(d, level)| name_line(d, level)).collect()
    }
}

fn name_line(d: &EnchantmentDef, level: i32) -> Vec<TextSpan> {
    let farbe = if CURSES.contains(&d.name.as_str()) { RED } else { GRAY };
    let mut spans = match &d.description {
        Some(desc) => protocol::nbt::text_to_spans(desc),
        None => vec![TextSpan { text: d.name.trim_start_matches("minecraft:").replace('_', " "), ..Default::default() }],
    };
    for span in &mut spans {
        span.color.get_or_insert(farbe);
    }
    if level != 1 || d.max_level != 1 {
        let key = format!("enchantment.level.{level}");
        let stufe = protocol::nbt::translate(&key).unwrap_or(key);
        spans.push(TextSpan { text: format!(" {stufe}"), color: Some(farbe), ..Default::default() });
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn compound(pairs: Vec<(&str, NbtValue)>) -> NbtValue {
        NbtValue::Compound(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<HashMap<_, _>>())
    }

    fn s(v: &str) -> NbtValue {
        NbtValue::String(v.to_string())
    }

    // quick_charge.json aus dem 1.21.11-Jar, riptide.json und binding_curse.json ebenso.
    fn vanilla() -> Enchantments {
        let linear = |base: f32, per: f32| {
            compound(vec![
                ("type", s("minecraft:linear")),
                ("base", NbtValue::Float(base)),
                ("per_level_above_first", NbtValue::Float(per)),
            ])
        };
        let add = |value: NbtValue| compound(vec![("type", s("minecraft:add")), ("value", value)]);
        let beschreibung = |key: &str| compound(vec![("translate", s(key))]);
        Enchantments::from_registry(vec![
            (
                "minecraft:quick_charge".into(),
                Some(compound(vec![
                    ("description", beschreibung("enchantment.minecraft.quick_charge")),
                    ("max_level", NbtValue::Int(3)),
                    ("effects", compound(vec![("minecraft:crossbow_charge_time", add(linear(-0.25, -0.25)))])),
                ])),
            ),
            (
                "minecraft:riptide".into(),
                Some(compound(vec![
                    ("description", beschreibung("enchantment.minecraft.riptide")),
                    ("max_level", NbtValue::Int(3)),
                    ("effects", compound(vec![("minecraft:trident_spin_attack_strength", add(linear(1.5, 0.75)))])),
                ])),
            ),
            (
                "minecraft:binding_curse".into(),
                Some(compound(vec![
                    ("description", beschreibung("enchantment.minecraft.binding_curse")),
                    ("max_level", NbtValue::Int(1)),
                    ("effects", compound(vec![("minecraft:prevent_armor_change", compound(vec![]))])),
                ])),
            ),
            (
                "minecraft:sharpness".into(),
                Some(compound(vec![("description", beschreibung("enchantment.minecraft.sharpness")), ("max_level", NbtValue::Int(5))])),
            ),
        ])
    }

    fn stack(enchantments: &[(u32, i32)]) -> ItemStack {
        let mut s = ItemStack::new(1, 1);
        s.enchantments = enchantments.iter().map(|&(id, level)| Enchantment { id, level }).collect();
        s
    }

    #[test]
    fn schnellladen_verkuerzt_die_armbrust_wie_vanilla() {
        let e = vanilla();
        assert_eq!(e.crossbow_pull_ticks(&stack(&[])), 25);
        assert_eq!(e.crossbow_pull_ticks(&stack(&[(0, 1)])), 20);
        assert_eq!(e.crossbow_pull_ticks(&stack(&[(0, 2)])), 15);
        assert_eq!(e.crossbow_pull_ticks(&stack(&[(0, 3)])), 10);
        // Stufe 5 (per Befehl): 1.25 - 1.25 = 0 -- geklemmt, nicht negativ.
        assert_eq!(e.crossbow_pull_ticks(&stack(&[(0, 5)])), 0);
    }

    #[test]
    fn sog_staerke_je_stufe() {
        let e = vanilla();
        assert_eq!(e.trident_spin_strength(&stack(&[])), 0.0);
        assert_eq!(e.trident_spin_strength(&stack(&[(1, 1)])), 1.5);
        assert_eq!(e.trident_spin_strength(&stack(&[(1, 3)])), 3.0);
    }

    #[test]
    fn bindung_nur_mit_dem_fluch() {
        let e = vanilla();
        assert!(e.prevents_armor_change(&stack(&[(2, 1)])));
        assert!(!e.prevents_armor_change(&stack(&[(3, 5)])));
        // Unbekannte ID (Registry noch nicht da / kaputt): kein Absturz, keine Wirkung.
        assert!(!e.prevents_armor_change(&stack(&[(99, 1)])));
    }

    #[test]
    fn gespeicherte_verzauberungen_wirken_nicht() {
        let e = vanilla();
        let mut buch = ItemStack::new(1, 1);
        buch.stored_enchantments = vec![Enchantment { id: 0, level: 3 }];
        assert_eq!(e.crossbow_pull_ticks(&buch), 25);
        assert_eq!(e.tooltip_lines(&buch).len(), 1, "anzeigen aber schon");
    }

    #[test]
    fn tooltip_reihenfolge_farbe_und_stufe() {
        let e = vanilla();
        // Item-Reihenfolge Schaerfe, Bindung -- Vanilla zeigt den Fluch zuerst.
        let zeilen = e.tooltip_lines(&stack(&[(3, 5), (2, 1)]));
        let text = |z: &Vec<TextSpan>| z.iter().map(|s| s.text.as_str()).collect::<String>();
        assert_eq!(zeilen.len(), 2);
        // Ohne Sprachtabelle bleiben die Schluessel roh -- die Stufe haengt trotzdem an.
        assert_eq!(text(&zeilen[0]), "enchantment.minecraft.binding_curse", "Stufe I bei Maximalstufe I faellt weg");
        assert_eq!(zeilen[0][0].color, Some(RED));
        assert!(text(&zeilen[1]).starts_with("enchantment.minecraft.sharpness "));
        assert_eq!(zeilen[1][0].color, Some(GRAY));
    }

    #[test]
    fn glint_regeln() {
        let mut s = ItemStack::new(1, 1);
        assert!(!has_glint(&s));
        s.enchantments = vec![Enchantment { id: 0, level: 1 }];
        assert!(has_glint(&s));
        s.glint_override = Some(false);
        assert!(!has_glint(&s), "gesetzter Override gewinnt");
        let mut ohne = ItemStack::new(1, 1);
        ohne.glint_override = Some(true);
        assert!(has_glint(&ohne));
    }

    // Schaerfe, Bann, Schutz und Explosionsschutz wie in den 1.21.11-Daten: nur der
    // bedingungslose Schaden zaehlt, Schutz nach Schadensart getrennt.
    #[test]
    fn schaden_und_schutz_aus_den_effekten() {
        let linear = |base: f32, per: f32| {
            compound(vec![("type", s("minecraft:linear")), ("base", NbtValue::Float(base)), ("per_level_above_first", NbtValue::Float(per))])
        };
        let add = |value: NbtValue| compound(vec![("type", s("minecraft:add")), ("value", value)]);
        let tag = |id: &str, expected: bool| compound(vec![("id", s(id)), ("expected", NbtValue::Byte(expected as i8))]);
        let quelle = |tags: Vec<NbtValue>| compound(vec![("condition", s("minecraft:damage_source_properties")), ("predicate", compound(vec![("tags", NbtValue::List(tags))]))]);
        let eintrag = |effect: NbtValue, req: Option<NbtValue>| {
            let mut v = vec![("effect", effect)];
            if let Some(r) = req {
                v.push(("requirements", r));
            }
            NbtValue::List(vec![compound(v)])
        };
        let def = |name: &str, key: &str, liste: NbtValue| (name.to_string(), Some(compound(vec![("max_level", NbtValue::Int(5)), ("effects", compound(vec![(key, liste)]))])));
        let e = Enchantments::from_registry(vec![
            def("minecraft:sharpness", "minecraft:damage", eintrag(add(linear(1.0, 0.5)), None)),
            def(
                "minecraft:smite",
                "minecraft:damage",
                eintrag(add(linear(2.5, 2.5)), Some(compound(vec![("condition", s("minecraft:entity_properties"))]))),
            ),
            def("minecraft:protection", "minecraft:damage_protection", eintrag(add(linear(1.0, 1.0)), Some(quelle(vec![tag("minecraft:bypasses_invulnerability", false)])))),
            def(
                "minecraft:blast_protection",
                "minecraft:damage_protection",
                eintrag(
                    add(linear(2.0, 2.0)),
                    Some(compound(vec![
                        ("condition", s("minecraft:all_of")),
                        ("terms", NbtValue::List(vec![quelle(vec![tag("minecraft:is_explosion", true), tag("minecraft:bypasses_invulnerability", false)])])),
                    ])),
                ),
            ),
        ]);
        let mut schwert = ItemStack::new(1, 1);
        schwert.enchantments = vec![Enchantment { id: 0, level: 5 }, Enchantment { id: 1, level: 5 }];
        assert_eq!(e.damage_bonus(&schwert, 7.0), 3.0, "Schaerfe V = +3, Bann zaehlt nicht");
        let mut brust = ItemStack::new(2, 1);
        brust.enchantments = vec![Enchantment { id: 2, level: 4 }, Enchantment { id: 3, level: 2 }];
        assert_eq!(e.protection(&brust, ProtectionKind::General), 4.0);
        assert_eq!(e.protection(&brust, ProtectionKind::Explosion), 4.0);
        assert_eq!(e.protection(&brust, ProtectionKind::Fire), 0.0);
    }
}
