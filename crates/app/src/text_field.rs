// Einzeiliges Eingabefeld mit Schreibcursor und Markierung, fuer Fenster wie den
// Server-Eintrag: Tippen ersetzt die Markierung, Shift+Pfeile ziehen sie auf, Strg+A markiert
// alles, die Maus setzt den Cursor, Ziehen markiert, Doppelklick nimmt das Wort.
//
// Reine Logik wie chat_input -- gezeichnet wird in main.rs, Textbreiten kommen als Funktion
// herein (s. index_at).

use crate::chat_input::Move;

#[derive(Debug, Default, Clone)]
pub struct TextField {
    text: String,
    // Byte-Offset, immer auf einer Zeichengrenze.
    cursor: usize,
    // Anfang der Markierung; markiert ist von hier bis zum Cursor.
    anchor: Option<usize>,
    // Wo der Cursor stand, bevor markiert wurde: dort bleibt der gezeichnete Cursor, solange
    // eine Markierung besteht (s. caret) -- wie im Chat, wo Markieren ihn nicht bewegt.
    rest: usize,
    max_chars: usize,
}

impl TextField {
    pub fn new(text: &str, max_chars: usize) -> Self {
        let text: String = text.chars().take(max_chars).collect();
        Self { cursor: text.len(), rest: text.len(), text, anchor: None, max_chars }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    // Gezeichnet wird caret(); der Cursor selbst nur in den Tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    // Wo der Cursor gezeichnet wird: ohne Markierung am Cursor, mit Markierung dort, wo er vor
    // dem Markieren stand.
    pub fn caret(&self) -> usize {
        if self.selection().is_some() { self.rest } else { self.cursor }
    }

    // Vor allem, was eine Markierung anfangen kann: die Cursorstelle davor festhalten.
    fn hold_caret(&mut self) {
        if self.selection().is_none() {
            self.rest = self.cursor;
        }
    }

    // Markierter Bereich als Byte-Bereich, None = keiner.
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        let a = self.anchor?;
        (a != self.cursor).then(|| a.min(self.cursor)..a.max(self.cursor))
    }

    pub fn selected_text(&self) -> &str {
        self.selection().map_or("", |r| &self.text[r])
    }

    // Markierung aufheben (Strg+D): der Cursor bleibt, wo er gezeichnet war (s. caret).
    pub fn deselect(&mut self) {
        if self.selection().is_some() {
            self.cursor = self.rest;
        }
        self.anchor = None;
    }

    pub fn select_all(&mut self) {
        self.hold_caret();
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    // Cursor bewegen; `extend` (Shift) zieht die Markierung auf, sonst endet sie. Ohne Shift
    // springt ein Pfeil bei bestehender Markierung an deren Rand, wie in jedem Textfeld.
    pub fn move_cursor(&mut self, m: Move, extend: bool) {
        self.hold_caret();
        if !extend {
            if let Some(r) = self.selection().filter(|_| matches!(m, Move::Left | Move::Right)) {
                self.cursor = if m == Move::Left { r.start } else { r.end };
                self.anchor = None;
                return;
            }
            self.anchor = None;
        } else {
            self.anchor.get_or_insert(self.cursor);
        }
        self.cursor = self.target(m);
    }

    fn target(&self, m: Move) -> usize {
        let t = &self.text;
        let i = self.cursor;
        match m {
            Move::Left => prev(t, i),
            Move::Right => next(t, i),
            Move::Home => 0,
            Move::End => t.len(),
            Move::WordLeft => {
                let mut i = i;
                while i > 0 && t[..i].ends_with(' ') {
                    i = prev(t, i);
                }
                while i > 0 && !t[..i].ends_with(' ') {
                    i = prev(t, i);
                }
                i
            }
            Move::WordRight => {
                let mut i = i;
                while i < t.len() && !t[i..].starts_with(' ') {
                    i = next(t, i);
                }
                while i < t.len() && t[i..].starts_with(' ') {
                    i = next(t, i);
                }
                i
            }
        }
    }

    // Tippen/Einfuegen: ersetzt die Markierung. Steuerzeichen (auch Tab, das Trennzeichen
    // in servers.conf) fallen weg, mehr als max_chars Zeichen gibt es nicht.
    pub fn insert(&mut self, s: &str) {
        let r = self.selection().unwrap_or(self.cursor..self.cursor);
        let rest = self.text.chars().count() - self.text[r.clone()].chars().count();
        let neu: String = s.chars().filter(|c| !c.is_control()).take(self.max_chars.saturating_sub(rest)).collect();
        self.text.replace_range(r.clone(), &neu);
        self.cursor = r.start + neu.len();
        self.anchor = None;
    }

    // Ruecktaste/Entf: die Markierung, sonst das Zeichen davor bzw. dahinter.
    pub fn backspace(&mut self) {
        let r = self.selection().unwrap_or(prev(&self.text, self.cursor)..self.cursor);
        self.remove(r);
    }

    pub fn delete_forward(&mut self) {
        let r = self.selection().unwrap_or(self.cursor..next(&self.text, self.cursor));
        self.remove(r);
    }

    // Strg+X: die Markierung herausnehmen.
    pub fn cut(&mut self) -> String {
        let Some(r) = self.selection() else { return String::new() };
        let raus = self.text[r.clone()].to_string();
        self.remove(r);
        raus
    }

    fn remove(&mut self, r: std::ops::Range<usize>) {
        self.text.replace_range(r.clone(), "");
        self.cursor = r.start;
        self.anchor = None;
    }

    // Maus gedrueckt bei Byte-Offset `i`: Cursor dorthin, von hier aus markiert ein Ziehen.
    // Mit Shift wird die bestehende Markierung bis hierher erweitert.
    pub fn press(&mut self, i: usize, extend: bool) {
        self.hold_caret();
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = Some(i);
        }
        self.cursor = i;
    }

    // Maus gezogen: der Cursor folgt, der Anker bleibt.
    pub fn drag_to(&mut self, i: usize) {
        self.hold_caret();
        self.cursor = i;
    }

    // Doppelklick: das Wort um `i` (ohne Leerzeichen).
    pub fn select_word_at(&mut self, i: usize) {
        self.hold_caret();
        let t = &self.text;
        let mut a = i;
        while a > 0 && !t[..a].ends_with(' ') {
            a = prev(t, a);
        }
        let mut b = i;
        while b < t.len() && !t[b..].starts_with(' ') {
            b = next(t, b);
        }
        self.anchor = Some(a);
        self.cursor = b;
    }
}

fn prev(t: &str, i: usize) -> usize {
    t[..i].chars().next_back().map_or(i, |c| i - c.len_utf8())
}

fn next(t: &str, i: usize) -> usize {
    t[i..].chars().next().map_or(i, |c| i + c.len_utf8())
}

// Welche Zeichengrenze in `text` der Stelle `x` (Pixel ab Textanfang) am naechsten liegt.
// `width_of` misst einen Textanfang.
pub fn index_at(text: &str, x: f32, width_of: impl Fn(&str) -> f32) -> usize {
    let mut best = (0, x.abs());
    for (i, c) in text.char_indices() {
        let ende = i + c.len_utf8();
        let d = (width_of(&text[..ende]) - x).abs();
        if d < best.1 {
            best = (ende, d);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tippen_ersetzt_markierung_und_shift_pfeile_markieren() {
        let mut f = TextField::new("play.example.net", 64);
        f.move_cursor(Move::Home, false);
        f.move_cursor(Move::Right, true);
        f.move_cursor(Move::Right, true);
        assert_eq!(f.selected_text(), "pl");
        f.insert("x");
        assert_eq!(f.text(), "xay.example.net");
        f.select_all();
        f.insert("a\tb");
        assert_eq!(f.text(), "ab");
    }

    #[test]
    fn ruecktaste_loescht_markierung_sonst_ein_zeichen() {
        let mut f = TextField::new("häuser", 64);
        f.backspace();
        assert_eq!(f.text(), "häuse");
        f.press(1, false);
        f.drag_to(3);
        assert_eq!(f.selected_text(), "ä");
        f.backspace();
        assert_eq!(f.text(), "huse");
        assert_eq!(f.cursor(), 1);
    }

    #[test]
    fn grenze_gilt_auch_beim_ersetzen() {
        let mut f = TextField::new("abc", 4);
        f.insert("xyz");
        assert_eq!(f.text(), "abcx");
        f.select_all();
        f.insert("12345");
        assert_eq!(f.text(), "1234");
    }

    #[test]
    fn doppelklick_und_mausposition() {
        let mut f = TextField::new("mein server", 64);
        f.select_word_at(7);
        assert_eq!(f.selected_text(), "server");
        // Jedes Zeichen 10 px breit: 24 px liegt naeher an 2 Zeichen als an 3.
        assert_eq!(index_at("abcd", 24.0, |s| s.chars().count() as f32 * 10.0), 2);
        assert_eq!(index_at("abcd", 99.0, |s| s.chars().count() as f32 * 10.0), 4);
        assert_eq!(index_at("abcd", -5.0, |s| s.chars().count() as f32 * 10.0), 0);
    }

    #[test]
    fn cursor_bleibt_beim_markieren_stehen() {
        let mut f = TextField::new("mein server", 64);
        f.press(2, false);
        assert_eq!(f.caret(), 2);
        f.drag_to(8);
        assert_eq!((f.cursor(), f.caret()), (8, 2));
        f.drag_to(0);
        assert_eq!(f.caret(), 2);
        f.move_cursor(Move::End, false);
        assert_eq!(f.caret(), 11);
        f.select_all();
        assert_eq!(f.caret(), 11, "Strg+A laesst ihn stehen");
        f.move_cursor(Move::Right, false);
        f.press(7, false);
        f.select_word_at(7);
        assert_eq!((f.selected_text(), f.caret()), ("server", 7), "Doppelklick: dort, wo geklickt wurde");
        f.insert("x");
        assert_eq!(f.caret(), f.cursor());
    }

    #[test]
    fn pfeil_ohne_shift_springt_an_den_rand_der_markierung() {
        let mut f = TextField::new("abcdef", 64);
        f.press(1, false);
        f.drag_to(4);
        f.move_cursor(Move::Left, false);
        assert_eq!((f.cursor(), f.selection()), (1, None));
    }
}
