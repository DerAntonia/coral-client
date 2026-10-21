// Rezeptbuch wie in Bedrock: eine Truhe links neben dem Inventar (bzw. der Werkbank), darin
// alle freigeschalteten Rezepte, die ins Craftingfeld passen, als ihr Ergebnis.
//
// - Linksklick craftet einmal, das Ergebnis landet direkt im Inventar.
// - Mit dem Stapel-Modifikator so viele, wie in einen Stapel des Ergebnisses passen.
// - Rechtsklick legt so viel wie moeglich ins Craftingfeld, craftet aber nicht.
// - Der Schalter oben sortiert alles gerade Craftbare nach vorn; aus ist die Reihenfolge fest.
//
// Gecraftet wird auf dem Server (place_recipe, dann Klicks auf das Ergebnisfeld) -- hier steht
// nur, was dafuer gebraucht wird: welche Rezepte es gibt, ob sie aus dem Vorrat gehen und wo
// alles auf dem Bildschirm sitzt.

use std::collections::HashMap;

use protocol::item::ItemStack;
use protocol::recipe::{Ingredient, RecipeBookAdd, RecipeEntry};

// Truhe in GUI-Pixeln: Kopfzeile, Reihen zu 18, Unterkante (Teile aus generic_54).
pub const COLS: usize = 9;
pub const ROWS: usize = 8;
pub const PANEL_W: f32 = 176.0;
pub const HEAD_H: f32 = 17.0;
pub const FOOT_H: f32 = 7.0;
pub const PANEL_H: f32 = HEAD_H + ROWS as f32 * 18.0 + FOOT_H;
// Abstand zum Inventarfenster.
pub const GAP: f32 = 2.0;
// Schalter "Craftbares zuerst" oben rechts in der Kopfzeile (recipe_book/filter_*, 26x16).
pub const FILTER_W: f32 = 26.0;
pub const FILTER_H: f32 = 16.0;
// Knopf zum Oeffnen im Inventarfenster (recipe_book/button, 20x18), wie in Vanilla.
pub const BUTTON_W: f32 = 20.0;
pub const BUTTON_H: f32 = 18.0;

#[derive(Default)]
pub struct RecipeBook {
    // Freigeschaltete Rezepte in der Reihenfolge des Servers.
    pub entries: Vec<RecipeEntry>,
    pub tags: HashMap<String, Vec<u32>>,
    pub open: bool,
    pub craftable_first: bool,
    // Erste sichtbare Reihe.
    pub scroll: usize,
}

// Ein sichtbarer Eintrag: Index in `entries` und wie viele Saetze der Vorrat hergibt (alle,
// die Truhe zeigt sie als Anzahl des Ergebnisses).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub index: usize,
    pub sets: i32,
}

impl RecipeBook {
    pub fn apply_add(&mut self, add: RecipeBookAdd) {
        if add.replace {
            self.entries.clear();
        }
        for (entry, _) in add.entries {
            match self.entries.iter_mut().find(|e| e.id == entry.id) {
                Some(alt) => *alt = entry,
                None => self.entries.push(entry),
            }
        }
    }

    pub fn apply_remove(&mut self, ids: &[i32]) {
        self.entries.retain(|e| !ids.contains(&e.id));
    }

    // Beim Verbindungsende: Rezepte und Tags gehoeren zum Server, die Bedienung bleibt.
    pub fn forget_server(&mut self) {
        self.entries.clear();
        self.tags.clear();
        self.scroll = 0;
    }

    fn items_of<'a>(&'a self, zutat: &'a Ingredient) -> &'a [u32] {
        match zutat {
            Ingredient::Items(ids) => ids,
            Ingredient::Tag(name) => self.tags.get(name.trim_start_matches('#')).map_or(&[], Vec::as_slice),
        }
    }

    // Wie viele volle Saetze der Vorrat (Item-ID -> Anzahl) fuer dieses Rezept hergibt, hoechstens
    // `bis`. Je Satz bekommt erst die Zutat mit den wenigsten passenden Items eins, und zwar von
    // dem, wovon noch am meisten da ist -- so nimmt "irgendein Brett" nicht das einzige Eichenbrett,
    // das eine andere Zutat genau braucht.
    // Teilen sich die Zutaten keine vorraetigen Items (der Normalfall), ist das ohne Schleife
    // zu haben: je Zutatenart ihr Vorrat geteilt durch wie oft sie im Rezept steht.
    pub fn sets_from(&self, entry: &RecipeEntry, vorrat: &HashMap<u32, i32>, bis: i32) -> i32 {
        let Some(zutaten) = &entry.requirements else { return 0 };
        if zutaten.is_empty() {
            return 0;
        }
        let da = |id: &u32| vorrat.get(id).copied().unwrap_or(0) > 0;
        let mut arten: Vec<(&[u32], i32)> = Vec::new();
        for z in zutaten {
            let items = self.items_of(z);
            match arten.iter_mut().find(|(a, _)| *a == items) {
                Some((_, n)) => *n += 1,
                None => arten.push((items, 1)),
            }
        }
        let getrennt = arten.iter().enumerate().all(|(i, (a, _))| {
            arten[i + 1..].iter().all(|(b, _)| !a.iter().filter(|id| da(id)).any(|id| b.contains(id)))
        });
        if getrennt {
            return arten
                .iter()
                .map(|(items, n)| items.iter().filter(|id| da(id)).map(|id| vorrat[id]).sum::<i32>() / n)
                .min()
                .unwrap_or(0)
                .min(bis);
        }
        let mut reihenfolge: Vec<&[u32]> = zutaten.iter().map(|z| self.items_of(z)).collect();
        reihenfolge.sort_by_key(|items| items.len());
        let mut rest = vorrat.clone();
        let mut saetze = 0;
        while saetze < bis {
            for items in &reihenfolge {
                let Some(&id) = items.iter().filter(|id| rest.get(*id).copied().unwrap_or(0) > 0).max_by_key(|id| rest[*id]) else {
                    return saetze;
                };
                *rest.get_mut(&id).expect("eben gefunden") -= 1;
            }
            saetze += 1;
        }
        saetze
    }

    // Was in der Truhe steht: Rezepte der Werkbank-Kategorien, die ins Feld passen und ueber
    // das Buch craftbar sind. Feste Reihenfolge: nach Kategorie, darin wie vom Server.
    pub fn shown(&self, kante: i32, vorrat: &HashMap<u32, i32>) -> Vec<Shown> {
        let mut out: Vec<Shown> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| (0..=3).contains(&e.category) && e.requirements.is_some() && e.display.fits_grid(kante))
            .map(|(index, e)| Shown { index, sets: self.sets_from(e, vorrat, i32::MAX) })
            .collect();
        out.sort_by_key(|s| self.entries[s.index].category);
        if self.craftable_first {
            out.sort_by_key(|s| s.sets == 0);
        }
        out
    }

    pub fn max_scroll(&self, anzahl: usize) -> usize {
        anzahl.div_ceil(COLS).saturating_sub(ROWS)
    }

    // Erste sichtbare Reihe, begrenzt auf die jetzige Liste (sie kann geschrumpft sein).
    pub fn row(&self, anzahl: usize) -> usize {
        self.scroll.min(self.max_scroll(anzahl))
    }

    pub fn scroll_by(&mut self, rasten: f64, anzahl: usize) {
        self.scroll = (self.row(anzahl) as f64 - rasten).clamp(0.0, self.max_scroll(anzahl) as f64) as usize;
    }
}

// Vorrat fuer die Craftbarkeit: nur unveraenderte Stapel (nicht beschaedigt, verzaubert oder
// umbenannt), wie Vanilla sie fuer das Buch zaehlt.
pub fn stock<'a>(stacks: impl IntoIterator<Item = &'a ItemStack>) -> HashMap<u32, i32> {
    let mut out = HashMap::new();
    for s in stacks {
        if s.damage.unwrap_or(0) > 0 || !s.enchantments.is_empty() || s.custom_name.is_some() {
            continue;
        }
        *out.entry(s.item_id).or_insert(0) += s.count;
    }
    out
}

// Bildschirm-Geometrie, alles in Bildschirmpixeln.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

impl Panel {
    // Links neben dem Fenster, oben buendig.
    pub fn beside(bg_x: f32, bg_y: f32, scale: f32) -> Self {
        Panel { x: bg_x - (PANEL_W + GAP) * scale, y: bg_y, scale }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + PANEL_W * self.scale && py < self.y + PANEL_H * self.scale
    }

    // Linke obere Ecke des Icons im sichtbaren Platz `i` (0..COLS*ROWS).
    pub fn slot(&self, i: usize) -> (f32, f32) {
        let s = self.scale;
        (self.x + (8.0 + (i % COLS) as f32 * 18.0) * s, self.y + (HEAD_H + 1.0 + (i / COLS) as f32 * 18.0) * s)
    }

    // Sichtbarer Platz unter dem Zeiger (mit dem Rand zwischen den Icons, wie bei Slots).
    pub fn slot_at(&self, px: f32, py: f32) -> Option<usize> {
        let s = self.scale;
        let gx = (px - self.x) / s - 7.0;
        let gy = (py - self.y) / s - HEAD_H;
        if gx < 0.0 || gy < 0.0 {
            return None;
        }
        let (c, r) = ((gx / 18.0) as usize, (gy / 18.0) as usize);
        (c < COLS && r < ROWS).then_some(r * COLS + c)
    }

    pub fn filter(&self) -> (f32, f32, f32, f32) {
        let s = self.scale;
        (self.x + (PANEL_W - 7.0 - FILTER_W) * s, self.y + 1.0 * s, FILTER_W * s, FILTER_H * s)
    }
}

// Wo der Knopf im Fenster sitzt (GUI-Pixel relativ zum Fenster), wie in Vanilla: im Inventar
// rechts neben der Figur, an der Werkbank links vom Feld.
pub fn button_offset(kante: i32) -> (f32, f32) {
    if kante == 2 { (104.0, 61.0) } else { (5.0, 34.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::recipe::{RecipeDisplay, SlotDisplay};

    fn eintrag(id: i32, zutaten: Vec<Ingredient>, kategorie: i32) -> RecipeEntry {
        RecipeEntry {
            id,
            display: RecipeDisplay::Shapeless { ingredients: vec![SlotDisplay::Empty; zutaten.len()], result: SlotDisplay::Item(1), station: SlotDisplay::Empty },
            group: None,
            category: kategorie,
            requirements: Some(zutaten),
        }
    }

    #[test]
    fn zaehlt_saetze_und_verteilt_knappe_items_zuerst() {
        let mut buch = RecipeBook::default();
        buch.tags.insert("minecraft:planks".into(), vec![11, 12]);
        // Braucht genau Eiche (11) und irgendein Brett: mit 1x Eiche und 1x Birke geht es einmal.
        let e = eintrag(1, vec![Ingredient::Tag("minecraft:planks".into()), Ingredient::Items(vec![11])], 3);
        let vorrat = HashMap::from([(11, 1), (12, 1)]);
        assert_eq!(buch.sets_from(&e, &vorrat, 64), 1);
        let viel = HashMap::from([(11, 10), (12, 3)]);
        assert_eq!(buch.sets_from(&e, &viel, 64), 6);
        assert_eq!(buch.sets_from(&e, &viel, 2), 2);
        // Getrennte Zutaten, eine davon doppelt (2 Bretter + 1 Stock): 9 Bretter, 7 Stoecke -> 4.
        let e = eintrag(2, vec![Ingredient::Tag("minecraft:planks".into()), Ingredient::Tag("minecraft:planks".into()), Ingredient::Items(vec![20])], 0);
        let vorrat = HashMap::from([(11, 5), (12, 4), (20, 7)]);
        assert_eq!(buch.sets_from(&e, &vorrat, i32::MAX), 4);
        assert_eq!(buch.sets_from(&e, &vorrat, 3), 3);
    }

    #[test]
    fn craftbares_zuerst_und_nur_was_ins_feld_passt() {
        let mut buch = RecipeBook {
            entries: vec![
                eintrag(1, vec![Ingredient::Items(vec![5])], 3),
                eintrag(2, vec![Ingredient::Items(vec![6])], 0),
                eintrag(3, vec![Ingredient::Items(vec![7]); 5], 3), // fuenf Zutaten: nur Werkbank
                eintrag(4, vec![Ingredient::Items(vec![8])], 6),    // Ofen-Kategorie
            ],
            ..Default::default()
        };
        let vorrat = HashMap::from([(5, 1)]);
        let ids = |v: Vec<Shown>| v.into_iter().map(|s| buch.entries[s.index].id).collect::<Vec<_>>();
        assert_eq!(ids(buch.shown(2, &vorrat)), vec![2, 1]);
        assert_eq!(ids(buch.shown(3, &vorrat)), vec![2, 1, 3]);
        buch.craftable_first = true;
        assert_eq!(ids(buch.shown(3, &vorrat)), vec![1, 2, 3]);
    }

    #[test]
    fn ersetzen_und_entfernen() {
        let mut buch = RecipeBook::default();
        buch.apply_add(RecipeBookAdd { entries: vec![(eintrag(1, vec![], 0), 0), (eintrag(2, vec![], 0), 0)], replace: true });
        buch.apply_add(RecipeBookAdd { entries: vec![(eintrag(3, vec![], 0), 0)], replace: false });
        buch.apply_remove(&[2]);
        assert_eq!(buch.entries.iter().map(|e| e.id).collect::<Vec<_>>(), vec![1, 3]);
        buch.apply_add(RecipeBookAdd { entries: vec![(eintrag(9, vec![], 0), 0)], replace: true });
        assert_eq!(buch.entries.len(), 1);
    }

    #[test]
    fn plaetze_der_truhe() {
        let p = Panel { x: 100.0, y: 50.0, scale: 2.0 };
        assert_eq!(p.slot(0), (116.0, 86.0));
        assert_eq!(p.slot_at(117.0, 87.0), Some(0));
        assert_eq!(p.slot_at(116.0 + 18.0 * 2.0, 86.0 + 18.0 * 2.0), Some(COLS + 1));
        assert_eq!(p.slot_at(101.0, 87.0), None);
    }
}
