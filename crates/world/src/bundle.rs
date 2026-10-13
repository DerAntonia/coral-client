// Buendel (BundleContents): wie voll sie sind und wie Einlegen und Herausnehmen wirken -- fuer die
// Vorhersage im Inventar, den Tooltip mit Fuellbalken und das Item-Modell (bundle/fullness).
//
// Das Gewicht ist ein Bruch: ein Stapel zaehlt mit Anzahl / Stapelgroesse (64 Steine fuellen ein
// Buendel, 16 Enderperlen auch, ein Schwert allein), ein Bienennest mit Bienen fuellt es ganz,
// ein Buendel im Buendel zaehlt mit seinem Inhalt plus 1/16. Exakt gerechnet, weil "voll" eine
// Gleichheit ist.

use protocol::item::ItemStack;

// Ein Bruch zaehler/nenner, stets gekuerzt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fraction {
    pub num: u64,
    pub den: u64,
}

fn ggt(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

impl Fraction {
    pub const ZERO: Fraction = Fraction { num: 0, den: 1 };
    pub const ONE: Fraction = Fraction { num: 1, den: 1 };

    pub fn new(num: u64, den: u64) -> Self {
        let g = ggt(num, den.max(1));
        Fraction { num: num / g, den: den.max(1) / g }
    }

    pub fn le(self, o: Fraction) -> bool {
        (self.num as u128) * (o.den as u128) <= (o.num as u128) * (self.den as u128)
    }

    // Fraction.mulAndTruncate: (bruch * n) abgeschnitten.
    pub fn mul_trunc(self, n: u64) -> u64 {
        self.num * n / self.den
    }

    // Wie oft passt `teil` ganz hinein (abgerundet)?
    pub fn div_floor(self, teil: Fraction) -> u64 {
        if teil.num == 0 {
            return u64::MAX;
        }
        ((self.num as u128 * teil.den as u128) / (self.den as u128 * teil.num as u128)) as u64
    }
}

impl std::ops::Add for Fraction {
    type Output = Fraction;
    fn add(self, o: Fraction) -> Fraction {
        let g = ggt(self.den, o.den);
        let den = self.den / g * o.den;
        Fraction::new(self.num * (den / self.den) + o.num * (den / o.den), den)
    }
}

// Nie unter null (ein ueberladenes Buendel gilt als voll).
impl std::ops::Sub for Fraction {
    type Output = Fraction;
    fn sub(self, o: Fraction) -> Fraction {
        let g = ggt(self.den, o.den);
        let den = self.den / g * o.den;
        Fraction::new((self.num * (den / self.den)).saturating_sub(o.num * (den / o.den)), den)
    }
}

impl std::ops::Mul<u64> for Fraction {
    type Output = Fraction;
    fn mul(self, n: u64) -> Fraction {
        Fraction::new(self.num * n, self.den)
    }
}

const BUNDLE_IN_BUNDLE: Fraction = Fraction { num: 1, den: 16 };

// Ist das ein Buendel (BundleItem, auch die gefaerbten)?
pub fn is_bundle(stack: &ItemStack) -> bool {
    crate::item_registry::item_name(stack.item_id).is_some_and(|n| n == "bundle" || n.ends_with("_bundle"))
}

// Der Inhalt eines Buendels (gesetzt, sonst der leere Standard).
pub fn contents(stack: &ItemStack) -> &[ItemStack] {
    stack.bundle_contents.as_deref().unwrap_or(&[])
}

// Gewicht EINES Items dieses Stapels.
fn item_weight(stack: &ItemStack) -> Fraction {
    if let Some(inhalt) = &stack.bundle_contents {
        return weight_of(inhalt) + BUNDLE_IN_BUNDLE;
    }
    if stack.bees.is_some_and(|n| n > 0) {
        return Fraction::ONE;
    }
    Fraction::new(1, crate::item_components::max_stack(stack) as u64)
}

pub fn weight_of(inhalt: &[ItemStack]) -> Fraction {
    inhalt.iter().fold(Fraction::ZERO, |acc, s| acc + item_weight(s) * s.count.max(0) as u64)
}

// Wie voll das Buendel ist (0..1).
pub fn weight(stack: &ItemStack) -> Fraction {
    weight_of(contents(stack))
}

// Item.canFitInsideContainerItems: Shulkerkisten nicht.
fn fits_inside(stack: &ItemStack) -> bool {
    crate::item_registry::item_name(stack.item_id).is_some_and(|n| !n.ends_with("shulker_box"))
}

// Wie viele von `stack` noch hineinpassen (BundleContents.Mutable.getMaxAmountToAdd).
pub fn max_to_add(bundle: &ItemStack, stack: &ItemStack) -> i32 {
    if stack.count <= 0 || !fits_inside(stack) {
        return 0;
    }
    let frei = Fraction::ONE - weight(bundle);
    (frei.div_floor(item_weight(stack)).min(stack.count as u64)) as i32
}

// Legt so viel von `stack` ein wie passt (BundleContents.Mutable.tryInsert): ein gleicher
// stapelbarer Eintrag wird ergaenzt und rueckt nach vorn, sonst kommt ein neuer nach vorn.
// Rueckgabe: wie viele eingelegt wurden; `stack` behaelt den Rest.
pub fn insert(bundle: &mut ItemStack, stack: &mut ItemStack) -> i32 {
    let n = max_to_add(bundle, stack);
    if n == 0 {
        return 0;
    }
    let inhalt = bundle.bundle_contents.get_or_insert_with(Vec::new);
    let gleich = inhalt.iter().position(|s| crate::item_components::max_stack(s) > 1 && s.stackable_with(stack));
    let eintrag = match gleich {
        Some(i) => {
            let mut e = inhalt.remove(i);
            e.count += n;
            e
        }
        None => ItemStack { count: n, ..stack.clone() },
    };
    inhalt.insert(0, eintrag);
    stack.count -= n;
    bundle.bundle_selected = -1;
    n
}

// Nimmt den gewaehlten Eintrag heraus (sonst den vordersten), ganz (Mutable.removeOne).
pub fn remove_one(bundle: &mut ItemStack) -> Option<ItemStack> {
    let gewaehlt = bundle.bundle_selected;
    let inhalt = bundle.bundle_contents.as_mut().filter(|i| !i.is_empty())?;
    let i = usize::try_from(gewaehlt).ok().filter(|i| *i < inhalt.len()).unwrap_or(0);
    let raus = inhalt.remove(i);
    bundle.bundle_selected = -1;
    Some(raus)
}

// BundleContents.getNumberOfItemsToShow: hoechstens 12 Plaetze (11, wenn ein Platz fuer "+N"
// gebraucht wird), auf ganze Viererreihen von unten aufgefuellt.
pub fn items_to_show(n: usize) -> usize {
    let max = if n > 12 { 11 } else { 12 };
    let luecke = if n.is_multiple_of(4) { 0 } else { 4 - n % 4 };
    n.min(max - luecke)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, n: i32) -> ItemStack {
        ItemStack::new(crate::item_registry::item_id_by_name(name).unwrap(), n)
    }

    #[test]
    fn gewicht_und_einlegen() {
        let mut b = s("bundle", 1);
        assert_eq!(weight(&b), Fraction::ZERO);
        let mut steine = s("stone", 40);
        assert_eq!(insert(&mut b, &mut steine), 40);
        assert_eq!(weight(&b), Fraction::new(40, 64));
        // 24 Steine passen noch, also 6 Perlen (je 1/16) -- 24/64 = 6/16.
        let mut perlen = s("ender_pearl", 10);
        assert_eq!(insert(&mut b, &mut perlen), 6);
        assert_eq!(perlen.count, 4);
        assert_eq!(weight(&b), Fraction::ONE, "voll");
        assert_eq!(insert(&mut b, &mut s("stone", 1)), 0);
        // Das zuletzt Eingelegte steht vorn und kommt zuerst wieder heraus.
        assert_eq!(remove_one(&mut b).map(|r| (r.item_id, r.count)), Some((s("ender_pearl", 1).item_id, 6)));
        // Gleiches kommt zum vorhandenen Eintrag und rueckt nach vorn.
        let mut mehr = s("stone", 4);
        insert(&mut b, &mut s("dirt", 1));
        insert(&mut b, &mut mehr);
        assert_eq!(contents(&b).iter().map(|x| x.count).collect::<Vec<_>>(), vec![44, 1]);
        // Ein Schwert fuellt ein leeres Buendel ganz, eine Shulkerkiste passt nie.
        assert_eq!(max_to_add(&s("bundle", 1), &s("diamond_sword", 1)), 1);
        assert_eq!(max_to_add(&s("bundle", 1), &s("shulker_box", 1)), 0);
        // Ein Buendel im Buendel: Inhalt plus 1/16.
        let mut innen = s("bundle", 1);
        insert(&mut innen, &mut s("stone", 32));
        let mut aussen = s("bundle", 1);
        assert_eq!(max_to_add(&aussen, &innen), 1);
        insert(&mut aussen, &mut innen);
        assert_eq!(weight(&aussen), Fraction::new(9, 16));
    }

    #[test]
    fn gezeigte_plaetze() {
        assert_eq!(items_to_show(1), 1);
        assert_eq!(items_to_show(5), 5);
        assert_eq!(items_to_show(12), 12);
        assert_eq!(items_to_show(13), 8, "13 Eintraege: 8 Items in zwei vollen Reihen ueber der +N-Reihe");
        assert_eq!(items_to_show(16), 11);
        assert_eq!(Fraction::new(1, 2).mul_trunc(94), 47);
    }
}
