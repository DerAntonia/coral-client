//! Wechselnde Zahlen bekommen festen Platz, damit der Rest der Zeile stehen bleibt.
//!
//! Die Schrift hat unterschiedlich breite Ziffern ("1" ist schmaler als "8"). Springt eine
//! Zahl hin und her, rutscht alles hinter ihr mit -- und bei mittigem oder rechtsbuendigem
//! Text die ganze Zeile. Feste Ziffernbreiten ueberall kommen nicht in Frage: ein frueherer
//! Versuch riss die schmale "1" in ruhigem Text sichtbar auseinander (s. font_atlas).
//!
//! Deshalb wird ERKANNT, welche Zahl sich tatsaechlich aendert. Ein Text wird dazu auf sein
//! Muster reduziert: jede Ziffernfolge wird ein Platzhalter ("Kills: 12" -> "Kills: _").
//! Taucht dasselbe Muster Bild fuer Bild mit anderen Ziffern auf, ist die Zahl darin
//! "wechselnd". Ab dann bekommt JEDE Stelle ihrer Ziffernfolgen die Breite der breitesten
//! Ziffer, und die Ziffer steht mittig darin: eine "1", wo eben eine "8" war, ruckt nichts
//! dahinter weg. Auch Folgen, die sich (noch) nicht geaendert haben, z.B. die Stunde einer
//! Uhr -- sonst stuende dort eine eng gesetzte "11" neben weit gesetzten Minuten.
//!
//! Nur bei GLEICHER Stellenzahl: wird aus "9" eine "10", darf die Zeile schubsen (so
//! gewuenscht) -- die Stellen fangen mit der neuen Anzahl neu an.
//!
//! Abschaltbar (Einstellungen, Schrift): dann steht jede Zahl in ihrer natuerlichen Breite.
//!
//! Nur eindeutige Muster zaehlen: zeigt ein Bild dasselbe Muster mehrmals (neun Mal
//! "Hotbar _"), sind das verschiedene ruhige Texte und keine wechselnde Zahl.

use rustc_hash::FxHashMap as HashMap;
use std::time::{Duration, Instant};

/// Nach so langer Abwesenheit vergisst ein Muster seinen reservierten Platz.
const FORGET_AFTER: Duration = Duration::from_secs(30);
/// Hoechstens so oft wird aufgeraeumt.
const PRUNE_EVERY: Duration = Duration::from_secs(1);
/// Platzhalter fuer eine Ziffernfolge im Muster -- ein Zeichen, das in echtem Text nicht
/// vorkommt.
const RUN: char = '\u{1}';

#[derive(Debug)]
struct Entry {
    /// Die Ziffernfolgen beim letzten Mal.
    runs: Vec<String>,
    /// Je Ziffernfolge die groesste bisher gesehene Breite jeder Stelle, seit die Folge ihre
    /// jetzige Stellenzahl hat.
    widths: Vec<Vec<f32>>,
    /// Hat sich schon einmal eine Ziffer geaendert?
    varied: bool,
    last_seen: Instant,
}

#[derive(Debug)]
pub struct StableNumbers {
    entries: HashMap<String, Entry>,
    last_prune: Option<Instant>,
    enabled: bool,
}

impl Default for StableNumbers {
    fn default() -> Self {
        Self { entries: HashMap::default(), last_prune: None, enabled: true }
    }
}

/// Eine Ziffernfolge: Zeichenindex von/bis (exklusiv).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    start: usize,
    end: usize,
}

fn runs_of(chars: &[char]) -> Vec<Run> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            out.push(Run { start, end: i });
        } else {
            i += 1;
        }
    }
    out
}

// Das Muster eines Textes, oder None, wenn er nichts zum Festhalten hat: ohne Ziffern, oder
// NUR eine Zahl -- dann steht nichts dahinter, das rutschen koennte (und wer eine einzelne
// Zahl aus einer groesseren Zeile zeichnet, hat die Zeile schon selbst ausgerichtet, s.
// App::build_server_hud).
fn pattern(chars: &[char], runs: &[Run]) -> Option<String> {
    let first = runs.first()?;
    if runs.len() == 1 && first.start == 0 && first.end == chars.len() {
        return None;
    }
    let mut p = String::with_capacity(chars.len());
    let mut i = 0;
    for r in runs {
        p.extend(&chars[i..r.start]);
        p.push(RUN);
        i = r.end;
    }
    p.extend(&chars[i..]);
    Some(p)
}

fn has_digits(text: &str) -> bool {
    text.bytes().any(|b| b.is_ascii_digit())
}

impl StableNumbers {
    /// Alles vergessen -- etwa nach einem Schriftwechsel, dann stimmen die Breiten nicht mehr.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn set_enabled(&mut self, an: bool) {
        self.enabled = an;
    }

    /// Breite eines Textes (in der Einheit von `advance`), mit reserviertem Platz.
    pub fn width(&self, text: &str, advance: impl Fn(char) -> f32) -> f32 {
        if !has_digits(text) {
            return text.chars().map(&advance).sum();
        }
        self.layout(text, advance).1
    }

    /// Startposition jedes Zeichens und die Gesamtbreite.
    pub fn layout(&self, text: &str, advance: impl Fn(char) -> f32) -> (Vec<f32>, f32) {
        let chars: Vec<char> = text.chars().collect();
        let natuerlich = |chars: &[char]| {
            let mut pos = Vec::with_capacity(chars.len());
            let mut pen = 0.0;
            for &c in chars {
                pos.push(pen);
                pen += advance(c);
            }
            (pos, pen)
        };
        if !self.enabled || !has_digits(text) {
            return natuerlich(&chars);
        }
        let runs = runs_of(&chars);
        let Some(entry) = pattern(&chars, &runs).and_then(|p| self.entries.get(&p)).filter(|e| e.varied) else {
            return natuerlich(&chars);
        };
        let breiteste = ('0'..='9').map(&advance).fold(0.0, f32::max);
        let mut pos = vec![0.0; chars.len()];
        let mut pen = 0.0;
        let mut i = 0;
        for (k, r) in runs.iter().enumerate() {
            while i < r.start {
                pos[i] = pen;
                pen += advance(chars[i]);
                i += 1;
            }
            // Andere Stellenzahl als gelernt: natuerlich (schubst).
            let stellen = entry.widths.get(k).filter(|w| w.len() == r.end - r.start);
            while i < r.end {
                let eigen = advance(chars[i]);
                let platz = stellen.map_or(eigen, |_| breiteste.max(eigen));
                pos[i] = pen + (platz - eigen) / 2.0;
                pen += platz;
                i += 1;
            }
        }
        while i < chars.len() {
            pos[i] = pen;
            pen += advance(chars[i]);
            i += 1;
        }
        (pos, pen)
    }

    /// Die Texte EINES Bildes (bzw. eines zusammengehoerigen Stapels davon) ansehen und
    /// lernen, welche Zahlen wechseln.
    pub fn observe<'a>(&mut self, texts: impl IntoIterator<Item = &'a str>, now: Instant, advance: impl Fn(char) -> f32) {
        if !self.enabled {
            return;
        }
        let mut stapel: HashMap<String, (usize, Vec<String>, Vec<Vec<f32>>)> = HashMap::default();
        for text in texts {
            if !has_digits(text) {
                continue;
            }
            let chars: Vec<char> = text.chars().collect();
            let runs = runs_of(&chars);
            let Some(p) = pattern(&chars, &runs) else { continue };
            let e = stapel.entry(p).or_insert_with(|| (0, Vec::new(), Vec::new()));
            e.0 += 1;
            if e.0 == 1 {
                e.1 = runs.iter().map(|r| chars[r.start..r.end].iter().collect()).collect();
                e.2 = runs.iter().map(|r| chars[r.start..r.end].iter().map(|&c| advance(c)).collect()).collect();
            }
        }
        for (p, (n, runs, widths)) in stapel {
            if n != 1 {
                continue;
            }
            match self.entries.get_mut(&p) {
                Some(e) => {
                    if e.runs != runs {
                        e.varied = true;
                    }
                    for (alt, neu) in e.widths.iter_mut().zip(widths) {
                        if alt.len() == neu.len() {
                            for (a, n) in alt.iter_mut().zip(neu) {
                                *a = a.max(n);
                            }
                        } else {
                            *alt = neu;
                        }
                    }
                    e.runs = runs;
                    e.last_seen = now;
                }
                None => {
                    self.entries.insert(p, Entry { runs, widths, varied: false, last_seen: now });
                }
            }
        }
        if self.last_prune.is_none_or(|t| now.saturating_duration_since(t) >= PRUNE_EVERY) {
            self.last_prune = Some(now);
            self.entries.retain(|_, e| now.saturating_duration_since(e.last_seen) < FORGET_AFTER);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Eine Schrift mit schmaler "1" wie DM Sans: "1" = 3, andere Ziffern = 6, sonst 5.
    fn adv(c: char) -> f32 {
        match c {
            '1' => 3.0,
            '0'..='9' => 6.0,
            _ => 5.0,
        }
    }

    fn bild(s: &mut StableNumbers, t: Instant, texte: &[&str]) {
        s.observe(texte.iter().copied(), t, adv);
    }

    #[test]
    fn ruhige_zahl_bleibt_natuerlich() {
        let mut s = StableNumbers::default();
        let t = Instant::now();
        bild(&mut s, t, &["Kills: 81"]);
        bild(&mut s, t, &["Kills: 81"]);
        assert_eq!(s.width("Kills: 81", adv), 7.0 * 5.0 + 6.0 + 3.0);
    }

    #[test]
    fn wechselnde_zahl_haelt_ihren_platz() {
        let mut s = StableNumbers::default();
        let t = Instant::now();
        bild(&mut s, t, &["75 %"]);
        bild(&mut s, t, &["71 %"]);
        // Die breiteste Form ("75") bleibt reserviert, auch wenn gerade "71" dasteht.
        assert_eq!(s.width("71 %", adv), s.width("75 %", adv));
        // Das "%" steht an derselben Stelle.
        let (a, _) = s.layout("75 %", adv);
        let (b, _) = s.layout("71 %", adv);
        assert_eq!(a[3], b[3]);
        // Die schmale "1" steht mittig im Platz der "5".
        assert_eq!(b[1], a[1] + 1.5);
    }

    #[test]
    fn andere_stellenzahl_schubst() {
        let mut s = StableNumbers::default();
        let t = Instant::now();
        bild(&mut s, t, &["9 %"]);
        bild(&mut s, t, &["10 %"]);
        bild(&mut s, t, &["8 %"]);
        // Gelernt ist gerade eine Stelle: zwei Stellen stehen natuerlich.
        assert_eq!(s.width("10 %", adv), 3.0 + 6.0 + 5.0 + 5.0);
        assert_eq!(s.width("8 %", adv), 6.0 + 5.0 + 5.0);
        // Mit der neuen Stellenzahl wird neu gelernt: "8" ist breiter als die "1".
        bild(&mut s, t, &["1 %"]);
        assert_eq!(s.width("1 %", adv), s.width("8 %", adv));
    }

    #[test]
    fn ruhige_folge_in_wechselnder_zeile_bekommt_auch_platz() {
        let mut s = StableNumbers::default();
        let t = Instant::now();
        bild(&mut s, t, &["11:21:36"]);
        bild(&mut s, t, &["11:21:37"]);
        // Die Stunde hat sich nie geaendert, steht aber genauso weit wie der Rest.
        let (pos, _) = s.layout("11:21:37", adv);
        assert_eq!(pos[1] - pos[0], 6.0);
    }

    #[test]
    fn abgeschaltet_bleibt_alles_natuerlich() {
        let mut s = StableNumbers::default();
        s.set_enabled(false);
        let t = Instant::now();
        bild(&mut s, t, &["75 %"]);
        bild(&mut s, t, &["71 %"]);
        assert_eq!(s.width("71 %", adv), 6.0 + 3.0 + 5.0 + 5.0);
    }

    #[test]
    fn gleiche_muster_im_selben_bild_sind_keine_wechselnde_zahl() {
        let mut s = StableNumbers::default();
        let t = Instant::now();
        for _ in 0..3 {
            bild(&mut s, t, &["Hotbar 1", "Hotbar 8"]);
        }
        assert_eq!(s.width("Hotbar 1", adv), 7.0 * 5.0 + 3.0);
    }

    #[test]
    fn reine_zahl_wird_nicht_festgehalten() {
        let mut s = StableNumbers::default();
        let t = Instant::now();
        bild(&mut s, t, &["8"]);
        bild(&mut s, t, &["1"]);
        assert_eq!(s.width("1", adv), 3.0);
    }

    #[test]
    fn vergisst_nach_einer_weile() {
        let mut s = StableNumbers::default();
        let t = Instant::now();
        bild(&mut s, t, &["75 %"]);
        bild(&mut s, t, &["71 %"]);
        bild(&mut s, t + FORGET_AFTER + PRUNE_EVERY, &[]);
        assert_eq!(s.width("71 %", adv), 6.0 + 3.0 + 5.0 + 5.0);
    }
}
