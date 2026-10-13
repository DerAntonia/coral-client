// Was die Item-Modell-Definitionen (assets::ItemDefinition) ueber einen Stapel wissen wollen:
// seine wirksamen Components (s. world::item_components) und, wo bekannt, die Umgebung --
// Anzeigeort, laufende Benutzung, Hand, Abklingzeit.

use assets::ItemModelInput;
use protocol::item::ItemStack;
use world::item_components::Components;

// Laufende Benutzung genau dieses Stapels: Ticks seit Beginn und Gesamtdauer.
#[derive(Clone, Copy, Debug, Default)]
pub struct UseState {
    pub ticks: f32,
    pub duration: f32,
    // Ladezeit der Armbrust in Ticks.
    pub pull_ticks: f32,
}

pub struct StackInput<'a> {
    pub stack: &'a ItemStack,
    components: Components<'a>,
    pub context: &'a str,
    // Die Verzauberungs-Registry (schickt der Server).
    pub enchantments: &'a world::enchantment::Enchantments,
    // Registry-Namen der Trim-Materialien nach ID.
    pub trim_materials: &'a [String],
    pub using: Option<UseState>,
    pub left_handed: bool,
    pub cooldown: f32,
    pub selected: bool,
    pub carried: bool,
    pub fishing_cast: bool,
}

impl<'a> StackInput<'a> {
    pub fn new(stack: &'a ItemStack, context: &'a str, enchantments: &'a world::enchantment::Enchantments, trim_materials: &'a [String]) -> Self {
        StackInput {
            stack,
            components: Components::of(stack),
            context,
            enchantments,
            trim_materials,
            using: None,
            left_handed: false,
            cooldown: 0.0,
            selected: false,
            carried: false,
            fishing_cast: false,
        }
    }
}

impl ItemModelInput for StackInput<'_> {
    fn display_context(&self) -> &str {
        self.context
    }

    fn using_item(&self) -> bool {
        self.using.is_some()
    }

    fn use_ticks(&self) -> f32 {
        self.using.map_or(0.0, |u| u.ticks)
    }

    fn use_duration(&self) -> f32 {
        self.using.map_or(0.0, |u| u.duration)
    }

    fn crossbow_pull_ticks(&self) -> f32 {
        self.using.map_or(25.0, |u| u.pull_ticks)
    }

    fn damage(&self) -> i32 {
        self.components.damage()
    }

    fn max_damage(&self) -> i32 {
        self.components.max_damage()
    }

    fn unbreakable(&self) -> bool {
        self.components.unbreakable()
    }

    fn count(&self) -> i32 {
        self.stack.count
    }

    fn max_stack_size(&self) -> i32 {
        self.components.max_stack_size()
    }

    fn custom_model_float(&self, index: usize) -> Option<f32> {
        self.components.custom_model_data()?.floats.get(index).copied()
    }

    fn custom_model_flag(&self, index: usize) -> Option<bool> {
        self.components.custom_model_data()?.flags.get(index).copied()
    }

    fn custom_model_string(&self, index: usize) -> Option<&str> {
        self.components.custom_model_data()?.strings.get(index).map(String::as_str)
    }

    fn has_component(&self, component: &str, ignore_default: bool) -> bool {
        let Some(id) = world::item_components::component_id(component) else { return false };
        if ignore_default {
            self.components.has_set(id)
        } else {
            self.components.has(id)
        }
    }

    fn enchantments(&self, stored: bool) -> Option<Vec<(String, i32)>> {
        let liste = self.components.enchantments(stored)?;
        Some(liste.iter().filter_map(|e| Some((self.enchantments.name(e.id)?.to_string(), e.level))).collect())
    }

    // Wie items/crossbow.json: eine Rakete unter den Geschossen zaehlt vor Pfeilen.
    fn charge_type(&self) -> &str {
        if self.stack.charged_projectiles.is_empty() {
            return "none";
        }
        let rakete = world::item_registry::item_id_by_name("firework_rocket");
        if self.stack.charged_projectiles.iter().any(|id| Some(*id) == rakete) {
            "rocket"
        } else {
            "arrow"
        }
    }

    fn trim_material(&self) -> Option<String> {
        let (material, _) = self.stack.trim?;
        self.trim_materials.get(usize::try_from(material).ok()?).cloned()
    }

    fn main_hand_left(&self) -> bool {
        self.left_handed
    }

    fn cooldown(&self) -> f32 {
        self.cooldown
    }

    fn selected(&self) -> bool {
        self.selected
    }

    fn carried(&self) -> bool {
        self.carried
    }

    fn fishing_rod_cast(&self) -> bool {
        self.fishing_cast
    }

    fn local_time(&self, pattern: &str) -> Option<String> {
        // Nur das Muster der Vanilla-Truhen ("MM-dd" fuer Weihnachten).
        (pattern == "MM-dd").then(|| chrono::Local::now().format("%m-%d").to_string())
    }

    fn bundle_has_selected(&self) -> bool {
        world::bundle::is_bundle(self.stack) && usize::try_from(self.stack.bundle_selected).is_ok_and(|i| i < world::bundle::contents(self.stack).len())
    }

    fn bundle_fullness(&self) -> f32 {
        let w = world::bundle::weight(self.stack);
        w.num as f32 / w.den as f32
    }

    fn potion(&self) -> Option<String> {
        let id = self.stack.potion.as_ref()?.potion?;
        world::potion::potion_registry_name(id).map(|n| if n.contains(':') { n.to_string() } else { format!("minecraft:{n}") })
    }
}
