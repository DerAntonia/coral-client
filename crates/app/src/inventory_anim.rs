// Inventar > Animation: die Hervorhebung unter dem Zeiger blendet weich ein und aus
// (Settings::slot_hover_fade_ms), und nach einem eigenen Klick fliegen verschobene Items zu
// ihrem neuen Platz (swap_anim_ms). Beides ist nur Darstellung: die Slots stehen laengst auf
// dem neuen Stand, das Ziel zeigt waehrend des Flugs nur, was schon vorher dort lag.

use crate::Instant;
use protocol::item::ItemStack;

// So lange nach einem eigenen Klick gilt die naechste Aenderung der Slots als seine Folge
// (ohne Vorhersage kommt sie erst mit der Antwort des Servers).
const CLICK_WINDOW_SECS: f32 = 1.0;

#[derive(Default)]
pub(crate) struct InventoryAnim {
    pub(crate) hover: HoverFade,
    pub(crate) flights: Flights,
}

// Je Slot ein Anteil 0..1, der zu 1 laeuft, solange der Zeiger auf ihm steht, sonst zu 0.
#[derive(Default)]
pub(crate) struct HoverFade {
    // Welcher Bildschirm (sync_id, s. step): ein anderer verwirft alles.
    key: Option<i32>,
    last: Option<Instant>,
    slots: Vec<(usize, f32)>,
}

impl HoverFade {
    // Ein Bild weiter. Rueckgabe: Slot und Deckkraft-Anteil (schon weich ein- und auslaufend).
    pub(crate) fn step(&mut self, key: i32, hovered: Option<usize>, now: Instant, fade_ms: u32) -> Vec<(usize, f32)> {
        if self.key != Some(key) {
            self.key = Some(key);
            self.slots.clear();
            self.last = None;
        }
        let dt = self.last.map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
        self.last = Some(now);
        let schritt = if fade_ms == 0 { 1.0 } else { dt * 1000.0 / fade_ms as f32 };
        if let Some(h) = hovered.filter(|h| !self.slots.iter().any(|(i, _)| i == h)) {
            self.slots.push((h, if fade_ms == 0 { 1.0 } else { 0.0 }));
        }
        for (i, anteil) in &mut self.slots {
            *anteil = if Some(*i) == hovered { (*anteil + schritt).min(1.0) } else { (*anteil - schritt).max(0.0) };
        }
        self.slots.retain(|&(i, anteil)| anteil > 0.0 || Some(i) == hovered);
        self.slots.iter().map(|&(i, anteil)| (i, smooth(anteil))).collect()
    }
}

// Wohin ein Item fliegt: in einen Slot oder an den Zeiger (Doppelklick sammelt).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Target {
    Slot(usize),
    Cursor,
}

#[derive(Clone, Debug)]
pub(crate) struct Flight {
    // Was fliegt, mit der fliegenden Anzahl als count.
    pub(crate) stack: ItemStack,
    pub(crate) from: usize,
    pub(crate) to: Target,
    // Teil eines Tauschs (zwei Slots wechseln ihre Items): dann auf einem Bogen, damit die
    // beiden aneinander vorbeifliegen (s. arc_point); sonst geradeaus.
    pub(crate) arc: bool,
    start: Instant,
}

#[derive(Default)]
pub(crate) struct Flights {
    key: Option<i32>,
    before: Vec<Option<ItemStack>>,
    cursor_before: Option<ItemStack>,
    click_at: Option<Instant>,
    pub(crate) list: Vec<Flight>,
}

impl Flights {
    // Ein eigener Klick ist raus: die naechste Aenderung der Slots wird animiert.
    pub(crate) fn click_sent(&mut self, now: Instant) {
        self.click_at = Some(now);
    }

    // Ein Bild weiter: Fertige weg, und hat sich nach einem eigenen Klick etwas geaendert,
    // die neuen Fluege dazu. `key` = Bildschirm (sync_id); ein anderer faengt neu an.
    pub(crate) fn step(&mut self, key: i32, slots: &[Option<ItemStack>], cursor: Option<&ItemStack>, now: Instant, ms: u32) {
        if self.key != Some(key) || ms == 0 {
            self.list.clear();
        }
        if self.key != Some(key) {
            self.key = Some(key);
            self.click_at = None;
            self.before = slots.to_vec();
            self.cursor_before = cursor.cloned();
            return;
        }
        self.list.retain(|f| now.saturating_duration_since(f.start).as_secs_f32() * 1000.0 < ms as f32);
        if self.before.as_slice() == slots && self.cursor_before.as_ref() == cursor {
            return;
        }
        let nach_klick = self.click_at.is_some_and(|t| now.saturating_duration_since(t).as_secs_f32() < CLICK_WINDOW_SECS);
        if nach_klick && ms > 0 {
            self.click_at = None;
            let neu = pair_moves(&self.before, slots, self.cursor_before.as_ref(), cursor);
            // Was jetzt wieder bewegt wurde, fliegt nicht mehr vom alten Klick weiter.
            let geaendert = |i: usize| self.before.get(i) != slots.get(i);
            self.list.retain(|f| !geaendert(f.from) && !matches!(f.to, Target::Slot(z) if geaendert(z)));
            let tausch = |from: usize, to: Target| neu.iter().any(|&(_, f2, t2)| t2 == Target::Slot(from) && to == Target::Slot(f2));
            let fluege: Vec<Flight> = neu.iter().map(|(stack, from, to)| Flight { stack: stack.clone(), from: *from, to: *to, arc: tausch(*from, *to), start: now }).collect();
            self.list.extend(fluege);
        }
        self.before = slots.to_vec();
        self.cursor_before = cursor.cloned();
    }

    // Wie weit ein Flug ist, 0..1, weich ein- und auslaufend.
    pub(crate) fn progress(f: &Flight, now: Instant, ms: u32) -> f32 {
        smooth((now.saturating_duration_since(f.start).as_secs_f32() * 1000.0 / ms.max(1) as f32).min(1.0))
    }

    // Wie viele Items gerade noch zu diesem Slot unterwegs sind.
    pub(crate) fn incoming(&self, slot: usize) -> i32 {
        self.list.iter().filter(|f| f.to == Target::Slot(slot)).map(|f| f.stack.count).sum()
    }
}

// Wie weit ein Flug zur Seite ausholt, als Anteil seiner Strecke.
const ARC_BULGE: f32 = 0.2;

// Punkt auf dem Bogen von `a` nach `b` bei Fortschritt t: die Gerade, in der Mitte um
// ARC_BULGE der Strecke zur Seite gebogen -- immer zur selben Seite der Flugrichtung. Die
// beiden Items eines Tauschs fliegen deshalb auf zwei Boegen aneinander vorbei, statt sich
// auf derselben Linie zu kreuzen.
pub(crate) fn arc_point(a: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let aus = (t * std::f32::consts::PI).sin() * ARC_BULGE;
    (a.0 + dx * t - dy * aus, a.1 + dy * t + dx * aus)
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

// Aus dem Stand vor und nach einem Klick: welche Items von wo nach wo gewandert sind. Je Slot,
// was dazukam und was wegging (bei anderem Item beides ganz); dann bekommt jedes Ziel seine
// Items aus den Slots, die dieselbe Sorte verloren haben. Am Ende der Zeiger, wenn er von
// seiner Sorte mehr wurde. Was keinen Partner hat (aufnehmen, ablegen, verteilen), fliegt nicht.
fn pair_moves(before: &[Option<ItemStack>], after: &[Option<ItemStack>], cursor_before: Option<&ItemStack>, cursor_after: Option<&ItemStack>) -> Vec<(ItemStack, usize, Target)> {
    let mut mehr: Vec<(Target, &ItemStack, i32)> = Vec::new();
    let mut weniger: Vec<(usize, &ItemStack, i32)> = Vec::new();
    for (i, (a, b)) in before.iter().zip(after).enumerate() {
        match (a, b) {
            (Some(a), Some(b)) if a.stackable_with(b) => {
                if b.count > a.count {
                    mehr.push((Target::Slot(i), b, b.count - a.count));
                } else if a.count > b.count {
                    weniger.push((i, a, a.count - b.count));
                }
            }
            _ => {
                if let Some(a) = a {
                    weniger.push((i, a, a.count));
                }
                if let Some(b) = b {
                    mehr.push((Target::Slot(i), b, b.count));
                }
            }
        }
    }
    if let (Some(a), Some(b)) = (cursor_before, cursor_after) {
        if a.stackable_with(b) && b.count > a.count {
            mehr.push((Target::Cursor, b, b.count - a.count));
        }
    }
    let mut out = Vec::new();
    for (ziel, item, mut bedarf) in mehr {
        for (von, quelle, rest) in &mut weniger {
            if bedarf == 0 {
                break;
            }
            if *rest == 0 || !quelle.stackable_with(item) || Target::Slot(*von) == ziel {
                continue;
            }
            let n = bedarf.min(*rest);
            *rest -= n;
            bedarf -= n;
            out.push((ItemStack { count: n, ..(*quelle).clone() }, *von, ziel));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn s(id: u32, n: i32) -> Option<ItemStack> {
        Some(ItemStack::new(id, n))
    }

    #[test]
    fn shift_klick_fliegt_von_quelle_zum_ziel() {
        // 4 fuellen den Stapel in Slot 2 auf, die anderen 6 landen im leeren Slot 1.
        let vorher = [s(1, 10), None, s(1, 60)];
        let nachher = [None, s(1, 6), s(1, 64)];
        let z: Vec<_> = pair_moves(&vorher, &nachher, None, None).into_iter().map(|(st, v, n)| (st.count, v, n)).collect();
        assert_eq!(z, vec![(6, 0, Target::Slot(1)), (4, 0, Target::Slot(2))]);
    }

    #[test]
    fn tausch_fliegt_in_beide_richtungen() {
        let vorher = [s(1, 3), s(2, 5)];
        let nachher = [s(2, 5), s(1, 3)];
        let mut z: Vec<_> = pair_moves(&vorher, &nachher, None, None).into_iter().map(|(st, v, n)| (st.item_id, st.count, v, n)).collect();
        z.sort_by_key(|e| e.0);
        assert_eq!(z, vec![(1, 3, 0, Target::Slot(1)), (2, 5, 1, Target::Slot(0))]);
    }

    #[test]
    fn nur_ein_tausch_fliegt_im_bogen() {
        let t0 = Instant::now();
        let mut f = Flights::default();
        f.step(0, &[s(1, 3), s(2, 5), None], None, t0, 200);
        f.click_sent(t0);
        f.step(0, &[s(2, 5), s(1, 3), None], None, t0, 200);
        assert!(f.list.len() == 2 && f.list.iter().all(|x| x.arc));
    }

    // Hin und zurueck bauchen zu entgegengesetzten Seiten aus, Start und Ziel bleiben.
    #[test]
    fn tausch_fliegt_auf_zwei_boegen() {
        let (a, b) = ((0.0, 0.0), (0.0, 100.0));
        assert_eq!(arc_point(a, b, 0.0), a);
        let ende = arc_point(a, b, 1.0);
        assert!((ende.0 - b.0).abs() < 1e-3 && (ende.1 - b.1).abs() < 1e-3);
        let hin = arc_point(a, b, 0.5);
        let zurueck = arc_point(b, a, 0.5);
        assert!((hin.0 + 20.0).abs() < 1e-3 && (zurueck.0 - 20.0).abs() < 1e-3);
        assert_eq!((hin.1, zurueck.1), (50.0, 50.0));
    }

    #[test]
    fn aufnehmen_und_ablegen_fliegen_nicht() {
        assert!(pair_moves(&[s(1, 8)], &[None], None, s(1, 8).as_ref()).is_empty());
        assert!(pair_moves(&[None], &[s(1, 8)], s(1, 8).as_ref(), None).is_empty());
    }

    #[test]
    fn doppelklick_sammelt_zum_zeiger() {
        let z = pair_moves(&[s(1, 5), s(1, 7)], &[None, None], s(1, 2).as_ref(), s(1, 14).as_ref());
        assert_eq!(z.iter().map(|(st, v, n)| (st.count, *v, *n)).collect::<Vec<_>>(), vec![(5, 0, Target::Cursor), (7, 1, Target::Cursor)]);
    }

    #[test]
    fn nur_nach_eigenem_klick_und_bis_zur_ankunft() {
        let t0 = Instant::now();
        let mut f = Flights::default();
        f.step(0, &[s(1, 4), None], None, t0, 200);
        // Ohne Klick (der Server schiebt etwas): kein Flug.
        f.step(0, &[None, s(1, 4)], None, t0, 200);
        assert!(f.list.is_empty());
        f.click_sent(t0);
        f.step(0, &[s(1, 4), None], None, t0, 200);
        assert_eq!(f.incoming(0), 4);
        assert!(!f.list[0].arc);
        f.step(0, &[s(1, 4), None], None, t0 + Duration::from_millis(250), 200);
        assert_eq!(f.incoming(0), 0);
    }

    #[test]
    fn hervorhebung_laeuft_ein_und_aus() {
        let t0 = Instant::now();
        let mut h = HoverFade::default();
        h.step(0, Some(3), t0, 100);
        let a = h.step(0, Some(3), t0 + Duration::from_millis(50), 100);
        assert_eq!(a.len(), 1);
        assert!((a[0].1 - 0.5).abs() < 0.01);
        let b = h.step(0, Some(4), t0 + Duration::from_millis(75), 100);
        assert!(b.iter().any(|&(i, x)| i == 3 && x > 0.0 && x < 0.5));
        let c = h.step(0, Some(4), t0 + Duration::from_millis(300), 100);
        assert_eq!(c, vec![(4, 1.0)]);
        // Sofort: ohne Uebergang.
        assert_eq!(h.step(0, Some(5), t0 + Duration::from_millis(301), 0), vec![(5, 1.0)]);
    }
}
