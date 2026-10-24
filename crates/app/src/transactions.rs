//! Transaktionen: die Ping-Pakete des Servers (ClientboundPing), die der Client sofort mit
//! derselben ID beantwortet. Vanilla schickt sie nie von selbst -- wer sie im Spiel schickt,
//! ist fast immer ein Anticheat, das daran misst, welchen Stand der Client gerade kennt. Wie
//! sich die IDs veraendern (Startwert, Schrittweite, Takt), ist je Anticheat verschieden.
//!
//! Hier nur Rohdaten und ein grob erkanntes Muster. Welches Muster zu welchem Anticheat
//! gehoert, steht bewusst NICHT hier: das soll aus eigenen Messungen auf Servern mit
//! bekanntem Anticheat kommen, nicht aus dem Gedaechtnis.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

// So viele letzte IDs fuer Muster und Rate.
const HISTORY: usize = 64;
// Ab so vielen gleichen Schritten in Folge gilt der Schritt als fest.
const STEADY_STEPS: usize = 8;

#[derive(Default)]
pub struct Transactions {
    ids: VecDeque<(Instant, i32)>,
    first: Option<i32>,
    total: u64,
    // Schon gemeldete Musterarten (s. kind) -- jede nur EINMAL je Verbindung. Frueher stand
    // hier nur die letzte Beschreibung samt Zaehlern ("13 verschiedene Schritte"), die sich
    // fast mit jedem Ping aendern und zwischen zwei Arten hin- und herspringen: das Log lief
    // mit Hunderten Zeilen voll.
    reported: Vec<String>,
}

impl Transactions {
    /// Neue ID. Liefert eine Log-Zeile beim ersten Ping und bei einer neuen Musterart.
    pub fn push(&mut self, id: i32, now: Instant) -> Option<String> {
        if self.ids.len() == HISTORY {
            self.ids.pop_front();
        }
        self.ids.push_back((now, id));
        self.total += 1;
        if self.first.is_none() {
            self.first = Some(id);
            return Some(format!("[transaktion] erste ID {id}"));
        }
        let art = self.kind();
        if self.ids.len() > STEADY_STEPS && !self.reported.contains(&art) {
            self.reported.push(art);
            let muster = self.pattern();
            return Some(format!("[transaktion] Muster: {muster} (erste ID {}, jetzt {id}, {} gesamt)", self.first.unwrap_or(id), self.total));
        }
        None
    }

    fn steps(&self) -> impl Iterator<Item = i64> + '_ {
        self.ids.iter().zip(self.ids.iter().skip(1)).map(|(a, b)| b.1 as i64 - a.1 as i64)
    }

    /// Die Art des Musters ohne die Zaehler, die sich laufend aendern: "zählt meist um -1, 12
    /// Sprünge" und "... 13 Sprünge" sind dieselbe Art, "wechselnd (13 ...)" und "(14 ...)"
    /// auch.
    fn kind(&self) -> String {
        let muster = self.pattern();
        match muster.find([',', '(']) {
            Some(i) => muster[..i].trim_end().to_string(),
            None => muster,
        }
    }

    /// Grob: fester Schritt, Schritt mit Ruecksprung (Ueberlauf/Neustart des Zaehlers),
    /// gleichbleibend oder wechselnd.
    pub fn pattern(&self) -> String {
        let schritte: Vec<i64> = self.steps().collect();
        let Some(&letzter) = schritte.last() else { return "noch zu wenig".to_string() };
        let hinten = schritte.iter().rev().take(STEADY_STEPS);
        if schritte.len() >= STEADY_STEPS && hinten.clone().all(|&s| s == letzter) {
            return match letzter {
                0 => "immer dieselbe ID".to_string(),
                s => format!("zählt um {s:+}"),
            };
        }
        // Haeufigster Schritt; ein seltener anderer ist meist ein Ueberlauf oder Neustart.
        let mut zaehler: Vec<(i64, usize)> = Vec::new();
        for &s in &schritte {
            match zaehler.iter_mut().find(|(w, _)| *w == s) {
                Some((_, n)) => *n += 1,
                None => zaehler.push((s, 1)),
            }
        }
        zaehler.sort_by_key(|z| std::cmp::Reverse(z.1));
        let (haupt, n) = zaehler[0];
        if n * 10 >= schritte.len() * 8 {
            format!("zählt meist um {haupt:+}, {} Sprünge", schritte.len() - n)
        } else {
            format!("wechselnd ({} verschiedene Schritte)", zaehler.len())
        }
    }

    /// Eine Zeile fuers Debug-Overlay.
    pub fn debug_line(&self, now: Instant) -> String {
        let Some(&(_, letzte)) = self.ids.back() else { return "Transaktionen: keine (kein Anticheat-Ping gesehen)".to_string() };
        let pro_s = self.ids.iter().filter(|(t, _)| now.duration_since(*t) < Duration::from_secs(1)).count();
        let schritt = self.steps().last().map_or(String::new(), |s| format!(", Schritt {s:+}"));
        format!(
            "Transaktionen: ID {letzte}{schritt}, {pro_s}/s, erste {}, {} gesamt  |  {}",
            self.first.unwrap_or(letzte),
            self.total,
            self.pattern()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mit(ids: impl IntoIterator<Item = i32>) -> Transactions {
        let mut t = Transactions::default();
        let jetzt = Instant::now();
        for id in ids {
            t.push(id, jetzt);
        }
        t
    }

    #[test]
    fn fester_schritt() {
        assert_eq!(mit((0..20).map(|i| -i)).pattern(), "zählt um -1");
        assert_eq!(mit((0..20).map(|i| 100 + 2 * i)).pattern(), "zählt um +2");
    }

    #[test]
    fn ueberlauf_ist_ein_sprung() {
        let ids: Vec<i32> = (0..20).map(|i| -(i % 15)).collect();
        assert_eq!(mit(ids).pattern(), "zählt meist um -1, 1 Sprünge");
    }

    #[test]
    fn meldet_erst_und_bei_wechsel() {
        let mut t = Transactions::default();
        let jetzt = Instant::now();
        assert!(t.push(5, jetzt).unwrap().contains("erste ID 5"));
        let zeilen: Vec<_> = (1..30).filter_map(|i| t.push(5 - i, jetzt)).collect();
        assert_eq!(zeilen.len(), 1, "{zeilen:?}");
        assert!(zeilen[0].contains("zählt um -1"));
    }

    #[test]
    fn jede_art_nur_einmal() {
        let mut t = Transactions::default();
        let jetzt = Instant::now();
        // Wild wechselnde IDs, dazwischen immer wieder lange gleiche Schritte -- wie im Log.
        let mut id = 0;
        let mut zeilen = Vec::new();
        for runde in 0..20 {
            for i in 0..40 {
                id += if runde % 2 == 0 { -1 } else { [7000, -7003, 5, -9][i % 4] };
                zeilen.extend(t.push(id, jetzt));
            }
        }
        // Erste ID plus hoechstens eine Zeile je Art.
        assert!(zeilen.len() <= 5, "{zeilen:#?}");
    }
}
