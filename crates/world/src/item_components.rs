// Die Components eines Stapels so, wie Vanilla sie sieht: was der Server gesetzt hat, sonst --
// wenn er sie nicht entfernt hat -- der Standard des Items.
//
// Ein Server schickt von einem Stapel nur die ABWEICHUNGEN (DataComponentPatch). Was ein Item
// von sich aus ist (die Elytra gleitet, das Totem rettet, der Schild blockt), steht im Jar; der
// Launcher extrahiert es einmal (assets ItemComponentsDump.java) in derselben Netzform, die ein
// Server schickt. Hier wird jede Standard-Component mit demselben Parser gelesen
// (protocol::item::apply_component) -- ein Stapel nur aus Standards, je Item einmal.
//
// Fehlen die extrahierten Daten (Launcher nie gelaufen), liefert `of` nur die gesetzten Werte;
// Aufrufer, die einen Standard brauchen, haben fuer diesen Fall ihre alte Tabelle.

use std::sync::OnceLock;

use protocol::item::{Equippable, ItemStack};

// Netz-IDs der Component-Typen in 1.21.11 (dieselbe Tabelle wie protocol::item).
pub const COMPONENT_MAX_STACK_SIZE: i32 = 1;
pub const COMPONENT_MAX_DAMAGE: i32 = 2;
pub const COMPONENT_DAMAGE: i32 = 3;
pub const COMPONENT_UNBREAKABLE: i32 = 4;
pub const COMPONENT_ENCHANTMENTS: i32 = 13;
pub const COMPONENT_CUSTOM_MODEL_DATA: i32 = 17;
pub const COMPONENT_STORED_ENCHANTMENTS: i32 = 41;
pub const COMPONENT_WRITABLE_BOOK_CONTENT: i32 = 52;

static DEFAULTS: OnceLock<Vec<Option<ItemStack>>> = OnceLock::new();
// Component-Name ("minecraft:glider") -> Netz-ID, aus demselben Dump.
static NAMES: OnceLock<std::collections::HashMap<String, i32>> = OnceLock::new();

// Liest den Dump (Format s. ItemComponentsDump.java). Rueckgabe: (Items, Components, die nicht
// lesbaren als "item component"). Nur der erste Aufruf zaehlt.
pub fn load(text: &str) -> (usize, usize, Vec<String>) {
    let mut tabelle: Vec<Option<ItemStack>> = Vec::new();
    let mut namen = std::collections::HashMap::new();
    let (mut items, mut gesamt, mut fehler) = (0, 0, Vec::new());
    let mut item_name = String::new();
    let mut aktuell: Option<ItemStack> = None;
    let ablegen = |stack: Option<ItemStack>, tabelle: &mut Vec<Option<ItemStack>>| {
        if let Some(s) = stack {
            let i = s.item_id as usize;
            if tabelle.len() <= i {
                tabelle.resize(i + 1, None);
            }
            tabelle[i] = Some(s);
        }
    };
    for zeile in text.lines() {
        if let Some(name) = zeile.strip_prefix("I ") {
            ablegen(aktuell.take(), &mut tabelle);
            item_name = name.to_string();
            let name = name.strip_prefix("minecraft:").unwrap_or(name);
            aktuell = crate::item_registry::item_id_by_name(name).map(|id| ItemStack::new(id, 1));
            items += aktuell.is_some() as usize;
        } else if let (Some(rest), Some(stack)) = (zeile.strip_prefix("C "), aktuell.as_mut()) {
            gesamt += 1;
            let mut felder = rest.split('\t');
            let kopf = felder.next().unwrap_or("");
            let hex = felder.next().unwrap_or("-");
            let Some(id) = kopf.rsplit(' ').next().and_then(|i| i.parse::<i32>().ok()) else {
                fehler.push(format!("{item_name} {kopf}"));
                continue;
            };
            if let Some(name) = kopf.split(' ').next() {
                namen.entry(name.to_string()).or_insert(id);
            }
            let Some(bytes) = dekodiere_hex(hex) else {
                fehler.push(format!("{item_name} {kopf}"));
                continue;
            };
            if let Err(e) = protocol::item::apply_component(stack, id, &bytes) {
                fehler.push(format!("{item_name} {kopf} {e:?} {hex}"));
            }
        }
    }
    ablegen(aktuell.take(), &mut tabelle);
    let _ = DEFAULTS.set(tabelle);
    let _ = NAMES.set(namen);
    (items, gesamt, fehler)
}

fn dekodiere_hex(hex: &str) -> Option<Vec<u8>> {
    if hex == "-" || !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect()
}

// Sind die Standards geladen?
pub fn loaded() -> bool {
    DEFAULTS.get().is_some_and(|t| !t.is_empty())
}

// Der Stapel nur aus den Standard-Components eines Items.
pub fn defaults(item_id: u32) -> Option<&'static ItemStack> {
    DEFAULTS.get()?.get(item_id as usize)?.as_ref()
}

// Die Netz-ID eines Component-Typs ("minecraft:glider" oder "glider"). Kennt nur Typen, die
// irgendein Item als Standard traegt -- die uebrigen fragt kein Item-Modell sinnvoll ab.
pub fn component_id(name: &str) -> Option<i32> {
    let namen = NAMES.get()?;
    match name.contains(':') {
        true => namen.get(name).copied(),
        false => namen.get(&format!("minecraft:{name}")).copied(),
    }
}

// Wie viele davon in einen Slot passen (max_stack_size, wirksam).
pub fn max_stack(stack: &ItemStack) -> i32 {
    Components::of(stack).max_stack_size().max(1)
}

// Die wirksamen Components eines Stapels.
#[derive(Clone, Copy)]
pub struct Components<'a> {
    stack: &'a ItemStack,
    default: Option<&'static ItemStack>,
}

impl<'a> Components<'a> {
    pub fn of(stack: &'a ItemStack) -> Self {
        Components { stack, default: defaults(stack.item_id) }
    }

    // Gesetzt -> der Wert; entfernt -> nichts; sonst der Standard.
    fn pick<T: ?Sized>(&self, id: i32, feld: impl for<'s> Fn(&'s ItemStack) -> Option<&'s T>) -> Option<&'a T> {
        if let Some(v) = feld(self.stack) {
            return Some(v);
        }
        if self.stack.removed_components.contains(&id) {
            return None;
        }
        self.default.and_then(feld)
    }

    pub fn equippable(&self) -> Option<&'a Equippable> {
        self.pick(protocol::item::COMPONENT_EQUIPPABLE, |s| s.equippable.as_ref())
    }

    // Traegt der Stapel die Component (gesetzt, oder Standard und nicht entfernt)?
    pub fn has(&self, id: i32) -> bool {
        self.stack.added_ids.contains(&id) || (!self.stack.removed_components.contains(&id) && self.default.is_some_and(|d| d.added_ids.contains(&id)))
    }

    // Vom Standard abweichend gesetzt (ItemStack.hasNonDefault)?
    pub fn has_set(&self, id: i32) -> bool {
        self.stack.added_ids.contains(&id)
    }

    pub fn damage(&self) -> i32 {
        self.pick(COMPONENT_DAMAGE, |s| s.damage.as_ref()).copied().unwrap_or(0)
    }

    // 0 = keine Haltbarkeit.
    pub fn max_damage(&self) -> i32 {
        self.pick(COMPONENT_MAX_DAMAGE, |s| s.max_damage.as_ref()).copied().unwrap_or(0)
    }

    pub fn unbreakable(&self) -> bool {
        self.has(COMPONENT_UNBREAKABLE)
    }

    // Ohne geladene Standards die Tabelle des Clients.
    pub fn max_stack_size(&self) -> i32 {
        match self.pick(COMPONENT_MAX_STACK_SIZE, |s| s.max_stack_size.as_ref()) {
            Some(n) => *n,
            None if self.default.is_some() => 1,
            None => crate::item_registry::max_stack_size(self.stack.item_id),
        }
    }

    // attack_range (Speere, Server-Waffen): eigene Angriffsreichweite statt der des Spielers.
    pub fn attack_range(&self) -> Option<&'a protocol::item::AttackRange> {
        self.pick(protocol::item::COMPONENT_ATTACK_RANGE, |s| s.attack_range.as_ref())
    }

    // Die wirksamen Attribut-Modifier (gesetzt oder Standard); leer ohne.
    pub fn attribute_modifiers(&self) -> &'a [protocol::item::ItemAttributeModifier] {
        self.pick(protocol::item::COMPONENT_ATTRIBUTE_MODIFIERS, |s| s.attribute_modifiers.as_deref()).unwrap_or(&[])
    }

    pub fn custom_model_data(&self) -> Option<&'a protocol::item::CustomModelData> {
        self.pick(COMPONENT_CUSTOM_MODEL_DATA, |s| s.custom_model_data.as_ref())
    }

    // Die enchantments- bzw. stored_enchantments-Component; None, wenn der Stapel sie nicht
    // traegt. Gesetzt kommt sie im Stapel an, der Standard ist bei allen Items leer.
    pub fn enchantments(&self, stored: bool) -> Option<&'a [protocol::item::Enchantment]> {
        let id = if stored { COMPONENT_STORED_ENCHANTMENTS } else { COMPONENT_ENCHANTMENTS };
        if !self.has(id) && self.default.is_some() {
            return None;
        }
        Some(if stored { &self.stack.stored_enchantments } else { &self.stack.enchantments })
    }
}

// Ein Verweis auf einen ganzen Stapel fuer Stellen, die Copy-Daten halten (HeldItem in der
// Entity-Tabelle) -- dort zaehlt fuers Bild auch, was erst die Item-Modelle aus den Components
// lesen (Verzauberung, custom_model_data ...). Die Stapel liegen in einem Ring fester Groesse;
// ein Verweis auf einen inzwischen ueberschriebenen Platz liefert nichts mehr (dann zeichnet der
// Aufrufer das gewoehnliche Modell des Items).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct StackRef(u32);

impl StackRef {
    pub const NONE: StackRef = StackRef(u32::MAX);
}

const RING: usize = 4096;

struct Ring {
    // Je Platz: laufende Nummer und Stapel.
    plaetze: Vec<Option<(u32, std::sync::Arc<ItemStack>)>>,
    // Inhalt -> laufende Nummer, damit gleiche Stapel denselben Verweis bekommen.
    nach_inhalt: std::collections::HashMap<Vec<u8>, u32>,
    naechste: u32,
}

static RING_STAPEL: std::sync::Mutex<Option<Ring>> = std::sync::Mutex::new(None);

// Was zwei Stapel fuers Bild gleich macht: Item, Anzahl und die Components wie auf der Leitung.
fn inhalt(s: &ItemStack) -> Vec<u8> {
    let mut k = Vec::with_capacity(16 + s.added_component_bytes.len());
    k.extend(s.item_id.to_le_bytes());
    k.extend(s.count.to_le_bytes());
    for r in &s.removed_components {
        k.extend(r.to_le_bytes());
    }
    k.push(0xff);
    k.extend(&s.added_component_bytes);
    k
}

pub fn intern(stack: &ItemStack) -> StackRef {
    let mut g = RING_STAPEL.lock().unwrap_or_else(|e| e.into_inner());
    let ring = g.get_or_insert_with(|| Ring { plaetze: vec![None; RING], nach_inhalt: std::collections::HashMap::new(), naechste: 0 });
    let key = inhalt(stack);
    if let Some(&nr) = ring.nach_inhalt.get(&key) {
        return StackRef(nr);
    }
    let nr = ring.naechste;
    // u32::MAX ist NONE.
    ring.naechste = if nr == u32::MAX - 1 { 0 } else { nr + 1 };
    let platz = nr as usize % RING;
    if let Some((alt, s)) = ring.plaetze[platz].take() {
        let alt_key = inhalt(&s);
        if ring.nach_inhalt.get(&alt_key) == Some(&alt) {
            ring.nach_inhalt.remove(&alt_key);
        }
    }
    ring.plaetze[platz] = Some((nr, std::sync::Arc::new(stack.clone())));
    ring.nach_inhalt.insert(key, nr);
    StackRef(nr)
}

pub fn resolve(r: StackRef) -> Option<std::sync::Arc<ItemStack>> {
    if r == StackRef::NONE {
        return None;
    }
    let g = RING_STAPEL.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref()?.plaetze[r.0 as usize % RING].as_ref()? {
        (nr, s) if *nr == r.0 => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stapel_verweise() {
        let mut a = ItemStack::new(5, 1);
        a.custom_model_data = Some(Default::default());
        a.added_component_bytes = vec![1, 2, 3];
        let r = intern(&a);
        assert_eq!(intern(&a.clone()), r, "gleicher Inhalt, gleicher Verweis");
        assert_eq!(resolve(r).as_deref(), Some(&a));
        let b = ItemStack::new(5, 2);
        assert_ne!(intern(&b), r);
        assert!(resolve(StackRef::NONE).is_none());
        // Nach einer vollen Runde ist der alte Platz ueberschrieben.
        for i in 0..RING as u32 {
            intern(&ItemStack::new(1000 + i, 1));
        }
        assert!(resolve(r).is_none());
        assert_ne!(intern(&a), r);
    }

    // Mit dem echten Dump aus dem Speicher des Launchers: alles muss sich lesen lassen, und die
    // Standards, auf die sich der Client stuetzt, muessen da sein. Braucht die extrahierten
    // Daten, deshalb #[ignore].
    #[test]
    #[ignore]
    fn echter_dump_wird_ganz_gelesen() {
        let pfad = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share/coral-client/storage/1.21.11/item_components.dat");
        let text = std::fs::read_to_string(pfad).unwrap();
        let (items, components, fehler) = load(&text);
        eprintln!("{items} Items, {components} Components, nicht lesbar: {fehler:#?}");
        assert!(items > 1400);
        assert!(fehler.is_empty(), "jede Standard-Component muss der Parser lesen koennen");
        let id = |n: &str| crate::item_registry::item_id_by_name(n).unwrap();
        let helm = ItemStack::new(id("diamond_helmet"), 1);
        let e = Components::of(&helm).equippable().expect("Helm traegt equippable");
        assert_eq!((e.slot, e.asset_id.as_deref()), (5, Some("minecraft:diamond")));
        assert!(Components::of(&ItemStack::new(id("stick"), 1)).equippable().is_none());
        // Vom Server gesetzt geht vor, entfernt heisst nichts.
        let mut uhc = helm.clone();
        uhc.equippable = Some(Equippable { slot: 5, asset_id: Some("uhc:invisible_helmet".into()), ..Default::default() });
        assert_eq!(Components::of(&uhc).equippable().unwrap().asset_id.as_deref(), Some("uhc:invisible_helmet"));
        let mut weg = helm.clone();
        weg.removed_components.push(protocol::item::COMPONENT_EQUIPPABLE);
        assert!(Components::of(&weg).equippable().is_none());
    }
}
