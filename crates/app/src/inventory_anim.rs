// Inventar > Animation: die Hervorhebung unter dem Zeiger blendet weich ein und aus
// (Settings::slot_hover_fade_ms), und nach einem Verschieben mit der Tastatur (Shift-Klick,
// Zifferntaste, Zweithand-Taste) fliegen die Items zu ihrem neuen Platz (swap_anim_ms).
// Aufnehmen, Ablegen und Verteilen mit der Maus fliegen nicht. Beides ist nur Darstellung: die
// Slots stehen laengst auf dem neuen Stand, das Ziel zeigt waehrend des Flugs nur, was schon
// vorher dort lag.

use crate::ui_anim::{ease_out_quint, HOVER_SETTLE};
use crate::Instant;
use protocol::item::ItemStack;

// So lange ohne Bild gilt ein Bildschirm als zu gewesen: was sich in der Zeit geaendert hat (etwa
// eine per Rechtsklick angezogene Ruestung), fliegt beim Oeffnen nicht nachtraeglich.
const NOT_DRAWN_SECS: f32 = 0.25;

// So lange nach einem Verschieben per Tastatur gilt die naechste Aenderung der Slots als seine
// Folge (ohne Vorhersage kommt sie erst mit der Antwort des Servers).
const CLICK_WINDOW_SECS: f32 = 1.0;

#[derive(Default)]
pub(crate) struct InventoryAnim {
    pub(crate) hover: HoverFade,
    pub(crate) flights: Flights,
}

// Je Slot: einblenden in fade_ms, solange der Zeiger auf ihm steht; danach klingt er aus wie
// die Knoepfe der Menues (Easing.OutQuint ueber HOVER_SETTLE, s. ui_anim) -- so leuchten beim
// Ueberstreichen mehrere Slots zugleich nach.
#[derive(Default)]
pub(crate) struct HoverFade {
    // Welcher Bildschirm (sync_id, s. step): ein anderer verwirft alles.
    key: Option<i32>,
    last: Option<Instant>,
    slots: Vec<SlotFade>,
}

struct SlotFade {
    index: usize,
    // Fortschritt des Einblendens 0..1.
    level: f32,
    // Seit wann er ausklingt und mit welcher Deckkraft das begann.
    out: Option<(Instant, f32)>,
}

impl SlotFade {
    fn shown(&self, now: Instant) -> f32 {
        match self.out {
            Some((seit, von)) => von * (1.0 - ease_out_quint(now.saturating_duration_since(seit).as_secs_f32() / HOVER_SETTLE.as_secs_f32())),
            None => smooth(self.level),
        }
    }
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
        if let Some(h) = hovered.filter(|h| !self.slots.iter().any(|s| s.index == *h)) {
            self.slots.push(SlotFade { index: h, level: if fade_ms == 0 { 1.0 } else { 0.0 }, out: None });
        }
        for s in &mut self.slots {
            if Some(s.index) == hovered {
                // Zurueck unter den Zeiger: von der Deckkraft aus weiter einblenden, die er hat.
                if s.out.is_some() {
                    s.level = inverse_smooth(s.shown(now));
                    s.out = None;
                }
                s.level = (s.level + schritt).min(1.0);
            } else if s.out.is_none() {
                s.out = Some((now, if fade_ms == 0 { 0.0 } else { s.shown(now) }));
            }
        }
        self.slots.retain(|s| s.out.is_none_or(|(seit, von)| von > 0.0 && now.saturating_duration_since(seit) < HOVER_SETTLE));
        self.slots.iter().map(|s| (s.index, s.shown(now))).collect()
    }
}

// Woher ein Item kommt und wohin es fliegt: ein Slot oder der Zeiger.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Target {
    Slot(usize),
    Cursor,
}

#[derive(Clone, Debug)]
pub(crate) struct Flight {
    // Was fliegt, mit der fliegenden Anzahl als count.
    pub(crate) stack: ItemStack,
    pub(crate) from: Target,
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
    // Wann zuletzt gezeichnet (s. NOT_DRAWN_SECS).
    last: Option<Instant>,
    click_at: Option<Instant>,
    pub(crate) list: Vec<Flight>,
}

impl Flights {
    // Ein Verschieben per Tastatur ist raus: die naechste Aenderung der Slots wird animiert.
    pub(crate) fn click_sent(&mut self, now: Instant) {
        self.click_at = Some(now);
    }

    // Ein Bild weiter: Fertige weg, und hat sich nach einem Verschieben per Tastatur etwas
    // geaendert, die neuen Fluege dazu. `key` = Bildschirm (sync_id); ein anderer faengt neu an.
    // `springt`: Items, die nie fliegen (Inventar > Außer Totems), sie stehen sofort am Ziel,
    // bei einem Tausch zusammen mit ihrem Partner.
    pub(crate) fn step(&mut self, key: i32, slots: &[Option<ItemStack>], cursor: Option<&ItemStack>, now: Instant, ms: u32, springt: impl Fn(&ItemStack) -> bool) {
        let zu_gewesen = self.last.is_none_or(|t| now.saturating_duration_since(t).as_secs_f32() > NOT_DRAWN_SECS);
        self.last = Some(now);
        let neu = self.key != Some(key) || zu_gewesen;
        if neu || ms == 0 {
            self.list.clear();
        }
        if neu {
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
            // Was jetzt wieder bewegt wurde, fliegt nicht mehr vom alten Stand weiter.
            let geaendert = |p: Target| match p {
                Target::Slot(i) => self.before.get(i) != slots.get(i),
                Target::Cursor => self.cursor_before.as_ref() != cursor,
            };
            self.list.retain(|f| !geaendert(f.from) && !geaendert(f.to));
            let tausch = |from: Target, to: Target| neu.iter().any(|&(_, f2, t2)| t2 == from && to == f2);
            // Ein Tausch mit einem springenden Item springt ganz: auch sein Partner fliegt nicht.
            let partner_springt = |from: Target, to: Target| neu.iter().any(|(s2, f2, t2)| *t2 == from && *f2 == to && springt(s2));
            let fluege: Vec<Flight> = neu.iter().filter(|(stack, from, to)| !springt(stack) && !partner_springt(*from, *to)).map(|(stack, from, to)| Flight { stack: stack.clone(), from: *from, to: *to, arc: tausch(*from, *to), start: now }).collect();
            self.list.extend(fluege);
        }
        self.before = slots.to_vec();
        self.cursor_before = cursor.cloned();
    }

    // Wie weit ein Flug ist, 0..1, weich ein- und auslaufend.
    pub(crate) fn progress(f: &Flight, now: Instant, ms: u32) -> f32 {
        smooth((now.saturating_duration_since(f.start).as_secs_f32() * 1000.0 / ms.max(1) as f32).min(1.0))
    }

    // Wie viele Items gerade noch zu diesem Platz unterwegs sind.
    pub(crate) fn incoming(&self, to: Target) -> i32 {
        self.list.iter().filter(|f| f.to == to).map(|f| f.stack.count).sum()
    }
}

// Wie weit ein Flug zur Seite ausholt, als Anteil seiner Strecke, wenn er eine Slotbreite weit
// geht. Weitere Fluege holen anteilig weniger aus (mit der Wurzel der Strecke in Slots).
const ARC_BULGE_NEAR: f32 = 0.3;

// Punkt auf dem Bogen von `a` nach `b` bei Fortschritt t: die Gerade, in der Mitte zur Seite
// gebogen -- immer zur selben Seite der Flugrichtung. Die beiden Items eines Tauschs fliegen
// deshalb auf zwei Boegen aneinander vorbei, statt sich auf derselben Linie zu kreuzen.
// `slot` ist die Slotgroesse in denselben Einheiten.
pub(crate) fn arc_point(a: (f32, f32), b: (f32, f32), t: f32, slot: f32) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let slots = (dx.hypot(dy) / slot.max(1.0)).max(1.0);
    let aus = (t * std::f32::consts::PI).sin() * ARC_BULGE_NEAR / slots.sqrt();
    (a.0 + dx * t - dy * aus, a.1 + dy * t + dx * aus)
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

// Umkehrung von smooth auf 0..1: welcher Fortschritt diese Deckkraft ergibt.
fn inverse_smooth(y: f32) -> f32 {
    0.5 - ((1.0 - 2.0 * y.clamp(0.0, 1.0)).asin() / 3.0).sin()
}

// Aus zwei Staenden: welche Items von wo nach wo gewandert sind. Je Platz (Slots und Zeiger),
// was dazukam und was wegging (bei anderem Item beides ganz); dann bekommt jedes Ziel seine
// Items von den Plaetzen, die dieselbe Sorte verloren haben. Was keinen Partner hat (vom Boden
// aufgehoben, weggeworfen, verbraucht), fliegt nicht.
fn pair_moves(before: &[Option<ItemStack>], after: &[Option<ItemStack>], cursor_before: Option<&ItemStack>, cursor_after: Option<&ItemStack>) -> Vec<(ItemStack, Target, Target)> {
    let mut mehr: Vec<Change> = Vec::new();
    let mut weniger: Vec<Change> = Vec::new();
    for (i, (a, b)) in before.iter().zip(after).enumerate() {
        compare(Target::Slot(i), a.as_ref(), b.as_ref(), &mut mehr, &mut weniger);
    }
    compare(Target::Cursor, cursor_before, cursor_after, &mut mehr, &mut weniger);
    let mut out = Vec::new();
    for (ziel, item, mut bedarf) in mehr {
        for (von, quelle, rest) in &mut weniger {
            if bedarf == 0 {
                break;
            }
            if *rest == 0 || !quelle.stackable_with(item) || *von == ziel {
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

type Change<'a> = (Target, &'a ItemStack, i32);

// Ein Platz vorher/nachher: was dazukam (`mehr`) und was wegging (`weniger`).
fn compare<'a>(platz: Target, a: Option<&'a ItemStack>, b: Option<&'a ItemStack>, mehr: &mut Vec<Change<'a>>, weniger: &mut Vec<Change<'a>>) {
    match (a, b) {
        (Some(a), Some(b)) if a.stackable_with(b) => {
            if b.count > a.count {
                mehr.push((platz, b, b.count - a.count));
            } else if a.count > b.count {
                weniger.push((platz, a, a.count - b.count));
            }
        }
        _ => {
            if let Some(a) = a {
                weniger.push((platz, a, a.count));
            }
            if let Some(b) = b {
                mehr.push((platz, b, b.count));
            }
        }
    }
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
        assert_eq!(z, vec![(6, Target::Slot(0), Target::Slot(1)), (4, Target::Slot(0), Target::Slot(2))]);
    }

    #[test]
    fn tausch_fliegt_in_beide_richtungen() {
        let vorher = [s(1, 3), s(2, 5)];
        let nachher = [s(2, 5), s(1, 3)];
        let mut z: Vec<_> = pair_moves(&vorher, &nachher, None, None).into_iter().map(|(st, v, n)| (st.item_id, st.count, v, n)).collect();
        z.sort_by_key(|e| e.0);
        assert_eq!(z, vec![(1, 3, Target::Slot(0), Target::Slot(1)), (2, 5, Target::Slot(1), Target::Slot(0))]);
    }

    #[test]
    fn nur_ein_tausch_fliegt_im_bogen() {
        let t0 = Instant::now();
        let mut f = Flights::default();
        f.step(0, &[s(1, 3), s(2, 5), None], None, t0, 200, |_| false);
        f.click_sent(t0);
        f.step(0, &[s(2, 5), s(1, 3), None], None, t0, 200, |_| false);
        assert!(f.list.len() == 2 && f.list.iter().all(|x| x.arc));
    }

    // Ausgenommene Items (Inventar > Außer Totems) springen, bei einem Tausch auch der Partner.
    #[test]
    fn ausgenommenes_item_springt_mit_tauschpartner() {
        let t0 = Instant::now();
        let mut f = Flights::default();
        f.step(0, &[s(1, 3), s(2, 5), None], None, t0, 200, |x| x.item_id == 1);
        f.click_sent(t0);
        f.step(0, &[s(2, 5), s(1, 3), None], None, t0, 200, |x| x.item_id == 1);
        assert!(f.list.is_empty());
    }

    // Ohne Tausch fliegt der Rest weiter, nur das ausgenommene Item springt.
    #[test]
    fn ausgenommenes_item_springt_allein() {
        let t0 = Instant::now();
        let mut f = Flights::default();
        f.step(0, &[s(1, 3), s(2, 5), None, None], None, t0, 200, |x| x.item_id == 1);
        f.click_sent(t0);
        f.step(0, &[None, None, s(1, 3), s(2, 5)], None, t0, 200, |x| x.item_id == 1);
        assert_eq!(f.list.len(), 1);
        assert_eq!(f.list[0].stack.item_id, 2);
    }

    // Hin und zurueck bauchen zu entgegengesetzten Seiten aus, Start und Ziel bleiben.
    #[test]
    fn tausch_fliegt_auf_zwei_boegen() {
        let (a, b) = ((0.0, 0.0), (0.0, 100.0));
        assert_eq!(arc_point(a, b, 0.0, 100.0), a);
        let ende = arc_point(a, b, 1.0, 100.0);
        assert!((ende.0 - b.0).abs() < 1e-3 && (ende.1 - b.1).abs() < 1e-3);
        let hin = arc_point(a, b, 0.5, 100.0);
        let zurueck = arc_point(b, a, 0.5, 100.0);
        assert!((hin.0 + 30.0).abs() < 1e-3 && (zurueck.0 - 30.0).abs() < 1e-3);
        assert_eq!((hin.1, zurueck.1), (50.0, 50.0));
    }

    // Ueber vier Slots: halb so viel Ausholen je Strecke wie ueber einen.
    #[test]
    fn weiter_bogen_holt_anteilig_weniger_aus() {
        let weit = arc_point((0.0, 0.0), (0.0, 400.0), 0.5, 100.0);
        assert!((weit.0 + 60.0).abs() < 1e-3);
    }

    #[test]
    fn umkehrung_von_smooth() {
        for y in [0.0, 0.1, 0.5, 0.9, 1.0] {
            assert!((smooth(inverse_smooth(y)) - y).abs() < 1e-4);
        }
    }

    #[test]
    fn aufnehmen_und_ablegen_fliegen() {
        let z = |v: Vec<(ItemStack, Target, Target)>| v.into_iter().map(|(st, von, nach)| (st.count, von, nach)).collect::<Vec<_>>();
        assert_eq!(z(pair_moves(&[s(1, 8)], &[None], None, s(1, 8).as_ref())), vec![(8, Target::Slot(0), Target::Cursor)]);
        assert_eq!(z(pair_moves(&[None], &[s(1, 8)], s(1, 8).as_ref(), None)), vec![(8, Target::Cursor, Target::Slot(0))]);
        // Verteilen: vom Zeiger in mehrere Slots.
        assert_eq!(z(pair_moves(&[None, None], &[s(1, 4), s(1, 4)], s(1, 8).as_ref(), None)), vec![(4, Target::Cursor, Target::Slot(0)), (4, Target::Cursor, Target::Slot(1))]);
    }

    #[test]
    fn vom_boden_und_verbraucht_fliegt_nicht() {
        assert!(pair_moves(&[None], &[s(1, 8)], None, None).is_empty());
        assert!(pair_moves(&[s(1, 8)], &[s(1, 7)], None, None).is_empty());
    }

    #[test]
    fn doppelklick_sammelt_zum_zeiger() {
        let z = pair_moves(&[s(1, 5), s(1, 7)], &[None, None], s(1, 2).as_ref(), s(1, 14).as_ref());
        assert_eq!(z.iter().map(|(st, v, n)| (st.count, *v, *n)).collect::<Vec<_>>(), vec![(5, Target::Slot(0), Target::Cursor), (7, Target::Slot(1), Target::Cursor)]);
    }

    #[test]
    fn nur_nach_tastatur_und_bis_zur_ankunft() {
        let t0 = Instant::now();
        let mut f = Flights::default();
        f.step(0, &[s(1, 4), None], None, t0, 200, |_| false);
        // Ohne Verschieben per Tastatur (Maus oder Server): kein Flug.
        f.step(0, &[None, s(1, 4)], None, t0, 200, |_| false);
        assert!(f.list.is_empty());
        f.click_sent(t0);
        f.step(0, &[s(1, 4), None], None, t0, 200, |_| false);
        assert_eq!(f.incoming(Target::Slot(0)), 4);
        assert!(!f.list[0].arc);
        f.step(0, &[s(1, 4), None], None, t0 + Duration::from_millis(210), 200, |_| false);
        assert_eq!(f.incoming(Target::Slot(0)), 0);
    }

    // Was sich bei geschlossenem Bildschirm geaendert hat, fliegt beim Oeffnen nicht.
    #[test]
    fn nach_dem_oeffnen_kein_nachtraeglicher_flug() {
        let t0 = Instant::now();
        let mut f = Flights::default();
        f.step(0, &[s(1, 1), s(2, 1)], None, t0, 200, |_| false);
        f.click_sent(t0 + Duration::from_secs(5));
        f.step(0, &[s(2, 1), s(1, 1)], None, t0 + Duration::from_secs(5), 200, |_| false);
        assert!(f.list.is_empty());
    }

    #[test]
    fn hervorhebung_laeuft_ein_und_aus() {
        let t0 = Instant::now();
        let mut h = HoverFade::default();
        h.step(0, Some(3), t0, 100);
        let a = h.step(0, Some(3), t0 + Duration::from_millis(50), 100);
        assert_eq!(a.len(), 1);
        assert!((a[0].1 - 0.5).abs() < 0.01);
        // Slot 3 klingt von 0.5 aus, waehrend 4 einblendet: beide zugleich hell.
        let b = h.step(0, Some(4), t0 + Duration::from_millis(75), 100);
        assert!(b.iter().any(|&(i, x)| i == 3 && (x - 0.5).abs() < 0.01));
        let c = h.step(0, Some(4), t0 + Duration::from_millis(300), 100);
        assert!(c.iter().any(|&(i, x)| i == 3 && x > 0.0 && x < 0.5));
        assert!(c.contains(&(4, 1.0)));
        let d = h.step(0, Some(4), t0 + Duration::from_millis(1000), 100);
        assert_eq!(d, vec![(4, 1.0)]);
        // Sofort: ohne Uebergang.
        assert_eq!(h.step(0, Some(5), t0 + Duration::from_millis(301), 0), vec![(5, 1.0)]);
    }
}
