// Item-Modell-Definitionen (assets/<ns>/items/<name>.json, seit 1.21.4) als Baum, der je Stapel
// ausgewertet wird -- wie Vanillas ItemModel-Arten:
//
//   model           ein Modell (Blatt), mit "tints"
//   special         Sondermodell (Kopf, Truhe, Schild ...) mit "base"-Modell (Blatt)
//   empty           nichts
//   composite       mehrere Modelle uebereinander
//   condition       Ja/Nein-Eigenschaft -> on_true / on_false
//   select          Eigenschaft mit Wert -> der erste passende Fall ("when"), sonst fallback
//   range_dispatch  Zahl * scale -> der Eintrag mit der groessten Schwelle <= Wert, sonst fallback
//   bundle/selected_item  das gewaehlte Item eines Buendels
//
// Die Blaetter werden beim Einlesen durchnummeriert (ItemDefinition::leaves): wer Modelle
// vorbereitet (Icon-Atlas, Hand-Modelle), kennt so alle, die je gebraucht werden koennen, und die
// Auswertung liefert nur noch Nummern.
//
// Was eine Eigenschaft ueber den Stapel und seine Umgebung wissen muss, fragt die Auswertung bei
// einem ItemModelInput ab -- dieses Crate kennt weder Stapel noch Welt.

use serde_json::Value;

use crate::blocks::strip_ns;

// Ein Blatt: das Modell (Pfad wie in den Modell-Dateien, "minecraft:" weggelassen) und seine
// Toenungsquellen. `special` ist die Art eines Sondermodells ("head", "chest", ...).
#[derive(Clone, Debug, PartialEq)]
pub struct Leaf {
    pub model: String,
    pub tints: Vec<Value>,
    pub special: Option<String>,
}

pub struct ItemDefinition {
    root: Node,
    pub leaves: Vec<Leaf>,
    // "hand_animation_on_swap" (Standard an) und "oversized_in_gui" (Standard aus).
    pub hand_animation_on_swap: bool,
    pub oversized_in_gui: bool,
    // Je Blatt der Schluessel seines Icons im UI-Atlas (s. ui_atlas::build), sonst leer.
    pub icon_keys: Vec<String>,
}

enum Node {
    Leaf(u32),
    Empty,
    Composite(Vec<Node>),
    Condition { prop: Condition, on_true: Box<Node>, on_false: Box<Node> },
    Select { prop: Select, cases: Vec<(Vec<Value>, Node)>, fallback: Option<Box<Node>> },
    Range { prop: Range, scale: f32, entries: Vec<(f32, Node)>, fallback: Option<Box<Node>> },
    BundleSelected,
}

enum Condition {
    UsingItem,
    Broken,
    Damaged,
    HasComponent { component: String, ignore_default: bool },
    FishingRodCast,
    BundleHasSelected,
    Selected,
    Carried,
    ExtendedView,
    KeybindDown(String),
    ViewEntity,
    CustomModelData(usize),
    Component { predicate: String, value: Value },
    Unknown,
}

enum Select {
    MainHand,
    ChargeType,
    TrimMaterial,
    BlockState(String),
    DisplayContext,
    LocalTime(String),
    ContextDimension,
    ContextEntityType,
    CustomModelData(usize),
    Component(String),
    Unknown,
}

enum Range {
    BundleFullness,
    Compass,
    Cooldown,
    Count { normalize: bool },
    CrossbowPull,
    CustomModelData(usize),
    Damage { normalize: bool },
    Time,
    UseCycle { period: f32 },
    UseDuration { remaining: bool },
    Unknown,
}

// Was die Eigenschaften ueber Stapel und Umgebung wissen muessen. Alles hat einen neutralen
// Standard (nicht benutzt, nicht gewaehlt, nichts gesetzt), damit ein Aufrufer nur liefert, was
// er kennt.
pub trait ItemModelInput {
    // "gui", "ground", "fixed", "head", "firstperson_righthand", "thirdperson_lefthand", ...
    fn display_context(&self) -> &str;
    // Wird genau dieser Stapel gerade benutzt? Dann die Ticks seither und die Gesamtdauer.
    fn using_item(&self) -> bool {
        false
    }
    fn use_ticks(&self) -> f32 {
        0.0
    }
    fn use_duration(&self) -> f32 {
        0.0
    }
    // Ladezeit der Armbrust in Ticks (Schnellladen verkuerzt sie).
    fn crossbow_pull_ticks(&self) -> f32 {
        25.0
    }
    // Schaden und Haltbarkeit; max_damage 0 = hat keine Haltbarkeit.
    fn damage(&self) -> i32 {
        0
    }
    fn max_damage(&self) -> i32 {
        0
    }
    fn unbreakable(&self) -> bool {
        false
    }
    fn count(&self) -> i32 {
        1
    }
    fn max_stack_size(&self) -> i32 {
        64
    }
    fn custom_model_float(&self, _index: usize) -> Option<f32> {
        None
    }
    fn custom_model_flag(&self, _index: usize) -> Option<bool> {
        None
    }
    fn custom_model_string(&self, _index: usize) -> Option<&str> {
        None
    }
    // Hat der Stapel die Component ("minecraft:glider")? Mit `ignore_default` nur, wenn sie
    // vom Standard des Items abweichend gesetzt ist.
    fn has_component(&self, _component: &str, _ignore_default: bool) -> bool {
        false
    }
    // Verzauberungen (voller Name, Stufe) -- `stored` fuer die eines Buchs. None = die
    // Component fehlt ganz.
    fn enchantments(&self, _stored: bool) -> Option<Vec<(String, i32)>> {
        None
    }
    // Was die Armbrust geladen hat: "none", "arrow" oder "rocket".
    fn charge_type(&self) -> &str {
        "none"
    }
    fn trim_material(&self) -> Option<String> {
        None
    }
    fn block_state_property(&self, _name: &str) -> Option<String> {
        None
    }
    // Der Benutzer ist Linkshaender.
    fn main_hand_left(&self) -> bool {
        false
    }
    // Restanteil der Abklingzeit 0..1.
    fn cooldown(&self) -> f32 {
        0.0
    }
    fn selected(&self) -> bool {
        false
    }
    fn carried(&self) -> bool {
        false
    }
    fn fishing_rod_cast(&self) -> bool {
        false
    }
    fn extended_view(&self) -> bool {
        false
    }
    fn view_entity(&self) -> bool {
        false
    }
    fn keybind_down(&self, _key: &str) -> bool {
        false
    }
    fn dimension(&self) -> Option<String> {
        None
    }
    fn entity_type(&self) -> Option<String> {
        None
    }
    // Ortszeit nach Javas DateTimeFormatter-Muster ("MM-dd").
    fn local_time(&self, _pattern: &str) -> Option<String> {
        None
    }
    // Tageszeit 0..1 (0 = Mittag wie Vanillas Tageszeit-Winkel) fuer "time".
    fn time_of_day(&self) -> f32 {
        0.0
    }
    // Der Trank einer potion_contents-Component ("minecraft:strong_healing").
    fn potion(&self) -> Option<String> {
        None
    }
    // Ist im Buendel ein Item gewaehlt (Mausrad), und wie voll ist es (0..1)?
    fn bundle_has_selected(&self) -> bool {
        false
    }
    fn bundle_fullness(&self) -> f32 {
        0.0
    }
    // Wert einer Component in JSON-Form fuer select auf "component" (nur einfache Werte).
    fn component_value(&self, _component: &str) -> Option<Value> {
        None
    }
}

const MAX_TIEFE: u32 = 64;

// Platzhalter in ItemDefinition::evaluate fuer bundle/selected_item.
pub const SELECTED_ITEM: u32 = u32::MAX;

impl ItemDefinition {
    // Liest die Datei (der Inhalt von items/<name>.json). None, wenn "model" fehlt.
    pub fn parse(json: &[u8]) -> Option<Self> {
        let wurzel: Value = serde_json::from_slice(crate::installation::strip_bom(json)).ok()?;
        let mut leaves = Vec::new();
        let root = parse_node(wurzel.get("model")?, &mut leaves, 0);
        Some(ItemDefinition {
            root,
            leaves,
            hand_animation_on_swap: wurzel.get("hand_animation_on_swap").and_then(Value::as_bool).unwrap_or(true),
            oversized_in_gui: wurzel.get("oversized_in_gui").and_then(Value::as_bool).unwrap_or(false),
            icon_keys: Vec::new(),
        })
    }

    // Die Blaetter, die fuer diesen Stapel gezeichnet werden (mehrere bei composite, keines bei
    // empty), unten zuerst. SELECTED_ITEM steht fuer das im Buendel gewaehlte Item, das der
    // Aufrufer an dieser Stelle selbst zeichnet.
    pub fn evaluate(&self, input: &dyn ItemModelInput) -> Vec<u32> {
        let mut out = Vec::new();
        evaluate(&self.root, input, &mut out, 0);
        out
    }
}

fn parse_node(v: &Value, leaves: &mut Vec<Leaf>, tiefe: u32) -> Node {
    if tiefe > MAX_TIEFE {
        return Node::Empty;
    }
    let Some(obj) = v.as_object() else { return Node::Empty };
    let art = obj.get("type").and_then(Value::as_str).map(strip_ns).unwrap_or("");
    let text = |k: &str| obj.get(k).and_then(Value::as_str).map(str::to_string);
    let prop_name = obj.get("property").and_then(Value::as_str).map(strip_ns).unwrap_or("").to_string();
    let index = obj.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    match art {
        "model" => {
            let Some(model) = text("model") else { return Node::Empty };
            let tints = obj.get("tints").and_then(Value::as_array).cloned().unwrap_or_default();
            leaves.push(Leaf { model: strip_ns(&model).to_string(), tints, special: None });
            Node::Leaf(leaves.len() as u32 - 1)
        }
        "special" => {
            let Some(base) = text("base") else { return Node::Empty };
            let special = obj.get("model").and_then(|m| m.get("type")).and_then(Value::as_str).map(|t| strip_ns(t).to_string());
            let tints = obj.get("tints").and_then(Value::as_array).cloned().unwrap_or_default();
            leaves.push(Leaf { model: strip_ns(&base).to_string(), tints, special: Some(special.unwrap_or_default()) });
            Node::Leaf(leaves.len() as u32 - 1)
        }
        "composite" => Node::Composite(
            obj.get("models").and_then(Value::as_array).map(|a| a.iter().map(|m| parse_node(m, leaves, tiefe + 1)).collect()).unwrap_or_default(),
        ),
        "condition" => {
            let prop = match prop_name.as_str() {
                "using_item" => Condition::UsingItem,
                "broken" => Condition::Broken,
                "damaged" => Condition::Damaged,
                "has_component" => Condition::HasComponent {
                    component: text("component").map(|c| full_id(&c)).unwrap_or_default(),
                    ignore_default: obj.get("ignore_default").and_then(Value::as_bool).unwrap_or(false),
                },
                "fishing_rod/cast" => Condition::FishingRodCast,
                "bundle/has_selected_item" => Condition::BundleHasSelected,
                "selected" => Condition::Selected,
                "carried" => Condition::Carried,
                "extended_view" => Condition::ExtendedView,
                "keybind_down" => Condition::KeybindDown(text("keybind").unwrap_or_default()),
                "view_entity" => Condition::ViewEntity,
                "custom_model_data" => Condition::CustomModelData(index),
                "component" => Condition::Component {
                    predicate: text("predicate").map(|p| strip_ns(&p).to_string()).unwrap_or_default(),
                    value: obj.get("value").cloned().unwrap_or(Value::Null),
                },
                _ => Condition::Unknown,
            };
            let on_true = Box::new(obj.get("on_true").map_or(Node::Empty, |n| parse_node(n, leaves, tiefe + 1)));
            let on_false = Box::new(obj.get("on_false").map_or(Node::Empty, |n| parse_node(n, leaves, tiefe + 1)));
            Node::Condition { prop, on_true, on_false }
        }
        "select" => {
            let prop = match prop_name.as_str() {
                "main_hand" => Select::MainHand,
                "charge_type" => Select::ChargeType,
                "trim_material" => Select::TrimMaterial,
                "block_state" => Select::BlockState(text("block_state_property").unwrap_or_default()),
                "display_context" => Select::DisplayContext,
                "local_time" => Select::LocalTime(text("pattern").unwrap_or_default()),
                "context_dimension" => Select::ContextDimension,
                "context_entity_type" => Select::ContextEntityType,
                "custom_model_data" => Select::CustomModelData(index),
                "component" => Select::Component(text("component").map(|c| full_id(&c)).unwrap_or_default()),
                _ => Select::Unknown,
            };
            let mut cases = Vec::new();
            for case in obj.get("cases").and_then(Value::as_array).into_iter().flatten() {
                let when = match case.get("when") {
                    Some(Value::Array(a)) => a.clone(),
                    Some(w) => vec![w.clone()],
                    None => continue,
                };
                let model = case.get("model").map_or(Node::Empty, |n| parse_node(n, leaves, tiefe + 1));
                cases.push((when, model));
            }
            let fallback = obj.get("fallback").map(|n| Box::new(parse_node(n, leaves, tiefe + 1)));
            Node::Select { prop, cases, fallback }
        }
        "range_dispatch" => {
            let prop = match prop_name.as_str() {
                "bundle/fullness" => Range::BundleFullness,
                "compass" => Range::Compass,
                "cooldown" => Range::Cooldown,
                "count" => Range::Count { normalize: obj.get("normalize").and_then(Value::as_bool).unwrap_or(true) },
                "crossbow/pull" => Range::CrossbowPull,
                "custom_model_data" => Range::CustomModelData(index),
                "damage" => Range::Damage { normalize: obj.get("normalize").and_then(Value::as_bool).unwrap_or(true) },
                "time" => Range::Time,
                "use_cycle" => Range::UseCycle { period: obj.get("period").and_then(Value::as_f64).unwrap_or(1.0) as f32 },
                "use_duration" => Range::UseDuration { remaining: obj.get("remaining").and_then(Value::as_bool).unwrap_or(false) },
                _ => Range::Unknown,
            };
            let scale = obj.get("scale").and_then(Value::as_f64).unwrap_or(1.0) as f32;
            let mut entries: Vec<(f32, Node)> = Vec::new();
            for e in obj.get("entries").and_then(Value::as_array).into_iter().flatten() {
                let schwelle = e.get("threshold").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                entries.push((schwelle, e.get("model").map_or(Node::Empty, |n| parse_node(n, leaves, tiefe + 1))));
            }
            // Aufsteigend, damit die Suche die groesste passende Schwelle findet.
            entries.sort_by(|a, b| a.0.total_cmp(&b.0));
            let fallback = obj.get("fallback").map(|n| Box::new(parse_node(n, leaves, tiefe + 1)));
            Node::Range { prop, scale, entries, fallback }
        }
        "bundle/selected_item" => Node::BundleSelected,
        _ => Node::Empty,
    }
}

// "glider" -> "minecraft:glider".
fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

// Gleiche Kennung, mit oder ohne "minecraft:".
fn same_id(a: &str, b: &str) -> bool {
    strip_ns(a) == strip_ns(b)
}

fn evaluate(node: &Node, input: &dyn ItemModelInput, out: &mut Vec<u32>, tiefe: u32) {
    if tiefe > MAX_TIEFE {
        return;
    }
    match node {
        Node::Leaf(i) => out.push(*i),
        Node::Empty => {}
        Node::BundleSelected => {
            if input.bundle_has_selected() {
                out.push(SELECTED_ITEM);
            }
        }
        Node::Composite(models) => {
            for m in models {
                evaluate(m, input, out, tiefe + 1);
            }
        }
        Node::Condition { prop, on_true, on_false } => {
            let zweig = if condition(prop, input) { on_true } else { on_false };
            evaluate(zweig, input, out, tiefe + 1);
        }
        Node::Select { prop, cases, fallback } => {
            let wert = select_value(prop, input);
            let fall = wert.as_ref().and_then(|w| cases.iter().find(|(when, _)| when.iter().any(|c| select_matches(prop, c, w))));
            match (fall, fallback) {
                (Some((_, m)), _) => evaluate(m, input, out, tiefe + 1),
                (None, Some(m)) => evaluate(m, input, out, tiefe + 1),
                (None, None) => {}
            }
        }
        Node::Range { prop, scale, entries, fallback } => {
            let wert = range_value(prop, input) * scale;
            let treffer = if wert.is_nan() { None } else { entries.iter().rev().find(|(s, _)| *s <= wert) };
            match (treffer, fallback) {
                (Some((_, m)), _) => evaluate(m, input, out, tiefe + 1),
                (None, Some(m)) => evaluate(m, input, out, tiefe + 1),
                (None, None) => {}
            }
        }
    }
}

fn damageable(input: &dyn ItemModelInput) -> bool {
    input.max_damage() > 0 && !input.unbreakable()
}

fn condition(prop: &Condition, input: &dyn ItemModelInput) -> bool {
    match prop {
        Condition::UsingItem => input.using_item(),
        // Ein Schlag vor dem Zerbrechen (Elytra: nicht mehr flugfaehig).
        Condition::Broken => damageable(input) && input.damage() >= input.max_damage() - 1,
        Condition::Damaged => damageable(input) && input.damage() > 0,
        Condition::HasComponent { component, ignore_default } => input.has_component(component, *ignore_default),
        Condition::FishingRodCast => input.fishing_rod_cast(),
        Condition::BundleHasSelected => input.bundle_has_selected(),
        Condition::Selected => input.selected(),
        Condition::Carried => input.carried(),
        Condition::ExtendedView => input.extended_view(),
        Condition::KeybindDown(k) => input.keybind_down(k),
        Condition::ViewEntity => input.view_entity(),
        Condition::CustomModelData(i) => input.custom_model_flag(*i).unwrap_or(false),
        Condition::Component { predicate, value } => component_predicate(predicate, value, input),
        Condition::Unknown => false,
    }
}

// Ein Component-Praedikat (dieselben wie in Befehlen und Loot-Tabellen) gegen den Stapel.
// Nicht umgesetzte Praedikate gelten als nicht erfuellt.
fn component_predicate(predicate: &str, value: &Value, input: &dyn ItemModelInput) -> bool {
    match predicate {
        "enchantments" | "stored_enchantments" => {
            let Some(liste) = input.enchantments(predicate == "stored_enchantments") else { return false };
            let preds: Vec<&Value> = match value {
                Value::Array(a) => a.iter().collect(),
                v => vec![v],
            };
            preds.iter().all(|p| enchantment_matches(p, &liste))
        }
        "damage" => {
            if !damageable(input) {
                return false;
            }
            let (schaden, max) = (input.damage(), input.max_damage());
            int_bounds(value.get("damage"), schaden) && int_bounds(value.get("durability"), max - schaden)
        }
        "potion_contents" => {
            let Some(trank) = input.potion() else { return false };
            match value {
                Value::String(s) => !s.starts_with('#') && same_id(s, &trank),
                Value::Array(a) => a.iter().filter_map(Value::as_str).any(|s| same_id(s, &trank)),
                _ => false,
            }
        }
        "trim" => {
            let Some(material) = input.trim_material() else { return false };
            match value.get("material") {
                None => true,
                Some(Value::String(s)) => same_id(s, &material),
                Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).any(|s| same_id(s, &material)),
                _ => false,
            }
        }
        _ => false,
    }
}

// EnchantmentPredicate: mit "enchantments" muss eine davon (mit passender Stufe) dran sein, ohne
// sie irgendeine mit passender Stufe, ohne beides irgendeine ueberhaupt.
fn enchantment_matches(pred: &Value, liste: &[(String, i32)]) -> bool {
    let stufe = pred.get("levels");
    match pred.get("enchantments") {
        Some(e) => {
            let namen: Vec<&str> = match e {
                Value::String(s) => vec![s.as_str()],
                Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            // Tags (#minecraft:...) kennt dieses Crate nicht -- sie treffen nie.
            liste.iter().any(|(n, l)| namen.iter().any(|w| !w.starts_with('#') && same_id(w, n)) && int_bounds(stufe, *l))
        }
        None if stufe.is_some() => liste.iter().any(|(_, l)| int_bounds(stufe, *l)),
        None => !liste.is_empty(),
    }
}

// MinMaxBounds.Ints: eine Zahl (genau) oder {"min", "max"}; fehlt sie, passt alles.
fn int_bounds(v: Option<&Value>, x: i32) -> bool {
    match v {
        None => true,
        Some(Value::Number(n)) => n.as_i64() == Some(x as i64),
        Some(o) => {
            let min = o.get("min").and_then(Value::as_i64);
            let max = o.get("max").and_then(Value::as_i64);
            min.is_none_or(|m| x as i64 >= m) && max.is_none_or(|m| x as i64 <= m)
        }
    }
}

// Der Wert, gegen den die Faelle eines select verglichen werden. None = keiner trifft.
fn select_value(prop: &Select, input: &dyn ItemModelInput) -> Option<Value> {
    let s = |t: &str| Some(Value::String(t.to_string()));
    match prop {
        Select::MainHand => s(if input.main_hand_left() { "left" } else { "right" }),
        Select::ChargeType => s(input.charge_type()),
        Select::TrimMaterial => input.trim_material().map(Value::String),
        Select::BlockState(name) => input.block_state_property(name).map(Value::String),
        Select::DisplayContext => s(input.display_context()),
        Select::LocalTime(pattern) => input.local_time(pattern).map(Value::String),
        Select::ContextDimension => input.dimension().map(Value::String),
        Select::ContextEntityType => input.entity_type().map(Value::String),
        Select::CustomModelData(i) => input.custom_model_string(*i).map(|t| Value::String(t.to_string())),
        Select::Component(c) => input.component_value(c),
        Select::Unknown => None,
    }
}

fn select_matches(prop: &Select, case: &Value, wert: &Value) -> bool {
    match (prop, case, wert) {
        // Kennungen duerfen mit oder ohne Namensraum stehen.
        (Select::TrimMaterial | Select::ContextDimension | Select::ContextEntityType | Select::Component(_), Value::String(a), Value::String(b)) => {
            same_id(a, b)
        }
        _ => case == wert,
    }
}

fn range_value(prop: &Range, input: &dyn ItemModelInput) -> f32 {
    let benutzt = input.using_item();
    match prop {
        Range::BundleFullness => input.bundle_fullness(),
        Range::Compass => 0.0,
        Range::Cooldown => input.cooldown(),
        Range::Count { normalize } => {
            let (n, max) = (input.count() as f32, input.max_stack_size() as f32);
            if *normalize {
                (n / max).clamp(0.0, 1.0)
            } else {
                n.clamp(0.0, max)
            }
        }
        Range::CrossbowPull if benutzt => {
            let pull = input.crossbow_pull_ticks();
            if pull <= 0.0 {
                f32::INFINITY
            } else {
                input.use_ticks() / pull
            }
        }
        Range::CrossbowPull => 0.0,
        Range::CustomModelData(i) => input.custom_model_float(*i).unwrap_or(0.0),
        Range::Damage { normalize } => {
            let (d, max) = (input.damage() as f32, input.max_damage() as f32);
            if *normalize {
                // Ohne Haltbarkeit 0/0 -- wie in Vanilla ergibt das keinen Wert (fallback).
                (d / max).clamp(0.0, 1.0)
            } else {
                d.clamp(0.0, max)
            }
        }
        Range::Time => input.time_of_day(),
        Range::UseCycle { period } if benutzt => (input.use_duration() - input.use_ticks()).max(0.0) % period.max(f32::MIN_POSITIVE),
        Range::UseCycle { .. } => 0.0,
        Range::UseDuration { remaining } if benutzt => {
            if *remaining {
                (input.use_duration() - input.use_ticks()).max(0.0)
            } else {
                input.use_ticks()
            }
        }
        Range::UseDuration { .. } => 0.0,
        Range::Unknown => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Probe {
        kontext: &'static str,
        verzauberungen: Option<Vec<(String, i32)>>,
        benutzt: Option<f32>,
        schaden: (i32, i32),
        cmd_floats: Vec<f32>,
        cmd_strings: Vec<String>,
        komponenten: Vec<&'static str>,
        ladung: &'static str,
    }

    impl ItemModelInput for Probe {
        fn display_context(&self) -> &str {
            self.kontext
        }
        fn using_item(&self) -> bool {
            self.benutzt.is_some()
        }
        fn use_ticks(&self) -> f32 {
            self.benutzt.unwrap_or(0.0)
        }
        fn damage(&self) -> i32 {
            self.schaden.0
        }
        fn max_damage(&self) -> i32 {
            self.schaden.1
        }
        fn enchantments(&self, _stored: bool) -> Option<Vec<(String, i32)>> {
            self.verzauberungen.clone()
        }
        fn custom_model_float(&self, i: usize) -> Option<f32> {
            self.cmd_floats.get(i).copied()
        }
        fn custom_model_string(&self, i: usize) -> Option<&str> {
            self.cmd_strings.get(i).map(String::as_str)
        }
        fn has_component(&self, c: &str, _ignore_default: bool) -> bool {
            self.komponenten.contains(&c)
        }
        fn charge_type(&self) -> &str {
            if self.ladung.is_empty() {
                "none"
            } else {
                self.ladung
            }
        }
    }

    fn modelle(def: &ItemDefinition, p: &Probe) -> Vec<String> {
        def.evaluate(p).into_iter().map(|i| def.leaves[i as usize].model.clone()).collect()
    }

    // Der Streitkolben aus "CPVP Aesthetic": je nach Verzauberung ein eigenes Modell.
    #[test]
    fn streitkolben_nach_verzauberung() {
        let def = ItemDefinition::parse(
            br#"{"model":{"type":"minecraft:condition","property":"minecraft:component","predicate":"minecraft:enchantments",
              "value":[{"enchantments":["minecraft:breach"]}],
              "on_true":{"type":"minecraft:model","model":"minecraft:item/mace_breach"},
              "on_false":{"type":"minecraft:condition","property":"minecraft:component","predicate":"minecraft:enchantments",
                "value":[{"enchantments":["minecraft:density"]}],
                "on_true":{"type":"minecraft:model","model":"minecraft:item/mace_density"},
                "on_false":{"type":"minecraft:model","model":"minecraft:item/mace"}}}}"#,
        )
        .unwrap();
        assert_eq!(def.leaves.len(), 3);
        let mit = |v: &[(&str, i32)]| Probe { kontext: "gui", verzauberungen: Some(v.iter().map(|(n, l)| (n.to_string(), *l)).collect()), ..Default::default() };
        assert_eq!(modelle(&def, &mit(&[])), ["item/mace"]);
        assert_eq!(modelle(&def, &mit(&[("minecraft:breach", 4)])), ["item/mace_breach"]);
        assert_eq!(modelle(&def, &mit(&[("minecraft:density", 5), ("minecraft:unbreaking", 3)])), ["item/mace_density"]);
        assert_eq!(modelle(&def, &mit(&[("minecraft:wind_burst", 1)])), ["item/mace"]);
        // Ohne Component (nicht geladene Standards) trifft das Praedikat nicht.
        assert_eq!(modelle(&def, &Probe { kontext: "gui", ..Default::default() }), ["item/mace"]);
    }

    #[test]
    fn stufen_und_leere_praedikate() {
        let l = vec![("minecraft:sharpness".to_string(), 3)];
        assert!(enchantment_matches(&serde_json::json!({}), &l));
        assert!(!enchantment_matches(&serde_json::json!({}), &[]));
        assert!(enchantment_matches(&serde_json::json!({"enchantments":"sharpness","levels":{"min":2}}), &l));
        assert!(!enchantment_matches(&serde_json::json!({"enchantments":"sharpness","levels":{"min":4}}), &l));
        assert!(!enchantment_matches(&serde_json::json!({"levels":5}), &l));
        assert!(!enchantment_matches(&serde_json::json!({"enchantments":"#minecraft:sharp"}), &l));
    }

    // Vanillas Bogen: range_dispatch auf use_duration mit scale 0.05.
    #[test]
    fn bogen_spannt_in_stufen() {
        let def = ItemDefinition::parse(
            br#"{"model":{"type":"condition","property":"using_item",
              "on_false":{"type":"model","model":"item/bow"},
              "on_true":{"type":"range_dispatch","property":"use_duration","scale":0.05,
                "entries":[{"threshold":0.9,"model":{"type":"model","model":"item/bow_pulling_2"}},
                           {"threshold":0.65,"model":{"type":"model","model":"item/bow_pulling_1"}}],
                "fallback":{"type":"model","model":"item/bow_pulling_0"}}}}"#,
        )
        .unwrap();
        let p = |t: Option<f32>| Probe { kontext: "gui", benutzt: t, ..Default::default() };
        assert_eq!(modelle(&def, &p(None)), ["item/bow"]);
        assert_eq!(modelle(&def, &p(Some(0.0))), ["item/bow_pulling_0"]);
        assert_eq!(modelle(&def, &p(Some(13.0))), ["item/bow_pulling_1"]);
        assert_eq!(modelle(&def, &p(Some(18.0))), ["item/bow_pulling_2"]);
    }

    #[test]
    fn select_composite_und_custom_model_data() {
        let def = ItemDefinition::parse(
            br#"{"model":{"type":"composite","models":[
              {"type":"select","property":"display_context","cases":[{"when":["gui","fixed"],"model":{"type":"model","model":"a:item/flach"}}],
               "fallback":{"type":"model","model":"item/hand"}},
              {"type":"select","property":"custom_model_data","cases":[{"when":"rot","model":{"type":"model","model":"item/rot"}}]},
              {"type":"range_dispatch","property":"custom_model_data","entries":[{"threshold":2,"model":{"type":"model","model":"item/zwei"}}]},
              {"type":"condition","property":"has_component","component":"glider","on_true":{"type":"model","model":"item/gleiter"},"on_false":{"type":"empty"}},
              {"type":"select","property":"charge_type","cases":[{"when":"rocket","model":{"type":"model","model":"item/rakete"}}]}]}}"#,
        )
        .unwrap();
        let p = Probe { kontext: "gui", cmd_strings: vec!["rot".into()], cmd_floats: vec![2.5], komponenten: vec!["minecraft:glider"], ladung: "rocket", ..Default::default() };
        assert_eq!(modelle(&def, &p), ["a:item/flach", "item/rot", "item/zwei", "item/gleiter", "item/rakete"]);
        assert_eq!(modelle(&def, &Probe { kontext: "thirdperson_righthand", ..Default::default() }), ["item/hand"]);
    }

    #[test]
    fn haltbarkeit() {
        let def = ItemDefinition::parse(
            br#"{"model":{"type":"condition","property":"broken","on_true":{"type":"model","model":"item/broken_elytra"},
              "on_false":{"type":"range_dispatch","property":"damage","entries":[{"threshold":0.5,"model":{"type":"model","model":"item/halb"}}],
                "fallback":{"type":"model","model":"item/elytra"}}}}"#,
        )
        .unwrap();
        let p = |d: i32| Probe { kontext: "gui", schaden: (d, 432), ..Default::default() };
        assert_eq!(modelle(&def, &p(0)), ["item/elytra"]);
        assert_eq!(modelle(&def, &p(300)), ["item/halb"]);
        assert_eq!(modelle(&def, &p(431)), ["item/broken_elytra"]);
    }
}
