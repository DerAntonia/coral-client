// Buecher lesen wie Vanillas BookViewScreen: das geschriebene Buch, das der Server per
// open_book oeffnet, und das Buch auf einem Lesepult (Menue "lectern", dort blaettert der
// Server mit).
//
// Aufbau in GUI-Pixeln (192x192 aus gui/book.png, oben mittig 2 px vom Rand): Text ab (36, 30),
// 114 breit, hoechstens 128/9 = 14 Zeilen zu 9 px, schwarz ohne Schatten; "Seite x von y"
// rechtsbuendig bis 148 auf Hoehe 16; Blaettern-Knoepfe (23x13) bei (43, 157) und (116, 157);
// darunter ein 200x20-Knopf "Fertig" (beim Lesepult zwei: "Fertig" und "Buch nehmen").

use protocol::item::ItemStack;
use protocol::nbt::TextSpan;

pub const IMAGE_W: f32 = 192.0;
pub const IMAGE_H: f32 = 192.0;
pub const TEXT_X: f32 = 36.0;
pub const TEXT_Y: f32 = 30.0;
pub const TEXT_W: f32 = 114.0;
pub const LINE_H: f32 = 9.0;
pub const MAX_LINES: usize = 128 / 9;
pub const INDICATOR_RIGHT: f32 = 148.0;
pub const INDICATOR_Y: f32 = 16.0;
pub const BUTTON_Y: f32 = 157.0;
pub const BACK_X: f32 = 43.0;
pub const FORWARD_X: f32 = 116.0;
pub const PAGE_BUTTON_W: f32 = 23.0;
pub const PAGE_BUTTON_H: f32 = 13.0;

// Ein offenes Buch aus der Hand.
pub struct BookView {
    pub pages: Vec<Vec<TextSpan>>,
    pub page: usize,
}

impl BookView {
    pub fn new(stack: &ItemStack) -> Self {
        BookView { pages: pages_of(stack), page: 0 }
    }

    pub fn back(&mut self) {
        self.page = self.page.saturating_sub(1);
    }

    pub fn forward(&mut self) {
        self.page = (self.page + 1).min(self.pages.len().saturating_sub(1));
    }
}

// Die Seiten eines Buchs (BookViewScreen.BookAccess.fromItem): die geschriebenen als Text, die
// eines Buchs mit Feder als Klartext. Mindestens eine (leere) Seite.
pub fn pages_of(stack: &ItemStack) -> Vec<Vec<TextSpan>> {
    let mut seiten: Vec<Vec<TextSpan>> = match (&stack.written_book, &stack.writable_pages) {
        (Some(b), _) => b.pages.clone(),
        (None, Some(p)) => p.iter().map(|t| vec![TextSpan { text: t.clone(), ..Default::default() }]).collect(),
        _ => Vec::new(),
    };
    if seiten.is_empty() {
        seiten.push(Vec::new());
    }
    seiten
}

// "Seite %1$s von %2$s" (book.pageIndicator) mit den Zahlen eingesetzt.
pub fn page_indicator(page: usize, total: usize) -> String {
    let muster = protocol::nbt::translate("book.pageIndicator").unwrap_or_else(|| "Page %1$s of %2$s".to_string());
    let (a, b) = ((page + 1).to_string(), total.max(1).to_string());
    if muster.contains("%1$s") || muster.contains("%2$s") {
        muster.replace("%1$s", &a).replace("%2$s", &b)
    } else {
        muster.replacen("%s", &a, 1).replacen("%s", &b, 1)
    }
}

// Wo das Bild liegt: oben links in Bildschirmpixeln bei GUI-Massstab `s`.
pub fn origin(screen_w: f32, s: f32) -> (f32, f32) {
    (((screen_w - IMAGE_W * s) / 2.0).floor(), 2.0 * s)
}

// --- Buch mit Feder schreiben (Vanillas BookEditScreen und BookSignScreen) -----------------
//
// Das Textfeld (MultiLineEditBox ohne Rahmen) liegt so, dass der Text bei (35, 30) im Bild
// beginnt, 114 GUI-Pixel breit, hoechstens 126/9 = 14 Zeilen und 1024 Zeichen je Seite,
// hoechstens 100 Seiten. "Weiter" auf der letzten Seite haengt eine leere an. Darunter
// "Signieren" und "Fertig" (je 98 breit). Fertig schickt die Seiten (ohne leere am Ende) per
// edit_book, Escape verwirft wie in Vanilla.
//
// Signieren (BookSignScreen): "Buchtitel eingeben:" mittig bei y 32 (im Bild), das Titelfeld
// (hoechstens 15 Zeichen, mittig, ohne Rahmen) bei 48, "von <Name>" dunkelgrau bei 58, der
// Hinweis umbrochen ab (36, 80). Knoepfe "Signieren und schliessen" (erst mit Titel) und
// "Abbrechen" (zurueck zum Schreiben, der Titel bleibt).
//
// Positionen sind Byte-Offsets in die Seite (Vanilla zaehlt UTF-16-Einheiten -- fuer die
// Grenzen zaehlen wir deshalb genauso, s. len16).

pub const MAX_PAGES: usize = 100;
pub const PAGE_CHAR_LIMIT: usize = 1024;
pub const EDIT_LINE_LIMIT: usize = 126 / 9;
pub const TITLE_LIMIT: usize = 15;
pub const EDIT_TEXT_X: f32 = 35.0;
pub const EDIT_TEXT_Y: f32 = 30.0;
// Mitte der Signier-Texte (36 + 114/2) und ihre Zeilen, jeweils relativ zum Bild.
pub const SIGN_CENTER_X: f32 = 93.0;
pub const SIGN_LABEL_Y: f32 = 32.0;
pub const SIGN_TITLE_Y: f32 = 48.0;
pub const SIGN_OWNER_Y: f32 = 58.0;
pub const SIGN_WARNING_X: f32 = 36.0;
pub const SIGN_WARNING_Y: f32 = 80.0;

// Laenge wie Javas String.length().
fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

// Hoechstens `max` UTF-16-Einheiten vom Anfang (StringUtil.truncateStringIfNecessary).
fn truncate16(s: &str, max: usize) -> &str {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        n += c.len_utf16();
        if n > max {
            return &s[..i];
        }
    }
    s
}

// StringUtil.isAllowedChatCharacter: kein Paragraphzeichen, keine Steuerzeichen, kein DEL.
pub fn allowed_char(c: char) -> bool {
    c != '\u{a7}' && c >= ' ' && c != '\u{7f}'
}

// Laengster Anfang, der in `max` Schriftpixel passt (Font.plainSubstrByWidth), als Byte-Laenge.
fn head_by_width(s: &str, max: f32, w: &dyn Fn(char) -> f32) -> usize {
    let mut breite = 0.0;
    for (i, c) in s.char_indices() {
        breite += w(c);
        if breite > max {
            return i;
        }
    }
    s.len()
}

pub fn width(s: &str, w: &dyn Fn(char) -> f32) -> f32 {
    s.chars().map(w).sum()
}

// StringSplitter.splitLines ohne Leerzeichen am Zeilenende: Bereiche der Zeilen. Umbrochen
// wird an '\n' und, sobald die Breite ueberschritten ist, am letzten Leerzeichen der Zeile
// (sonst vor dem Zeichen, das nicht mehr passt). Das Leerzeichen bzw. '\n' am Bruch gehoert
// zu keiner Zeile.
pub fn split_lines(text: &str, max_w: f32, w: &dyn Fn(char) -> f32) -> Vec<(usize, usize)> {
    let max_w = max_w.max(1.0);
    let mut zeilen = Vec::new();
    let mut pos = 0;
    while pos < text.len() {
        let (mut breite, mut sichtbar, mut leerzeichen, mut bruch) = (0.0, false, None, None);
        for (i, c) in text[pos..].char_indices() {
            let i = pos + i;
            if c == '\n' {
                bruch = Some(i);
                break;
            }
            if c == ' ' {
                leerzeichen = Some(i);
            }
            let cw = w(c);
            breite += cw;
            if sichtbar && breite > max_w {
                bruch = Some(leerzeichen.unwrap_or(i));
                break;
            }
            sichtbar |= cw != 0.0;
        }
        let Some(b) = bruch else {
            zeilen.push((pos, text.len()));
            break;
        };
        zeilen.push((pos, b));
        pos = if matches!(text.as_bytes()[b], b'\n' | b' ') { b + 1 } else { b };
    }
    zeilen
}

// Die angezeigten Zeilen (MultilineTextField.reflowDisplayLines): leerer Text ist eine leere
// Zeile, ein '\n' am Ende beginnt noch eine.
pub fn display_lines(text: &str, w: &dyn Fn(char) -> f32) -> Vec<(usize, usize)> {
    if text.is_empty() {
        return vec![(0, 0)];
    }
    let mut zeilen = split_lines(text, TEXT_W, w);
    if text.ends_with('\n') {
        zeilen.push((text.len(), text.len()));
    }
    zeilen
}

fn overflows_lines(text: &str, w: &dyn Fn(char) -> f32) -> bool {
    split_lines(text, TEXT_W, w).len() + usize::from(text.ends_with('\n')) > EDIT_LINE_LIMIT
}

// Tasten des Textfelds (MultilineTextField.keyPressed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Backspace,
    Delete,
    Enter,
}

pub struct BookEdit {
    // Wohin edit_book geht: Hotbar-Platz 0..8 bzw. 40 fuer die Zweithand.
    pub slot: i32,
    pub pages: Vec<String>,
    pub page: usize,
    cursor: usize,
    // Anderes Ende der Markierung; gleich dem Cursor = nichts markiert.
    anchor: usize,
    // Shift gehalten bzw. Ziehen mit der Maus: Cursor-Spruenge erweitern die Markierung.
    selecting: bool,
    // Offen ist gerade das Signieren.
    pub signing: bool,
    pub title: String,
}

impl BookEdit {
    pub fn new(slot: i32, mut pages: Vec<String>) -> Self {
        if pages.is_empty() {
            pages.push(String::new());
        }
        let mut b = BookEdit { slot, pages, page: 0, cursor: 0, anchor: 0, selecting: false, signing: false, title: String::new() };
        b.load_page();
        b
    }

    pub fn text(&self) -> &str {
        &self.pages[self.page]
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    // Markierter Bereich, wenn es einen gibt.
    pub fn selection(&self) -> Option<(usize, usize)> {
        (self.cursor != self.anchor).then(|| (self.cursor.min(self.anchor), self.cursor.max(self.anchor)))
    }

    pub fn selected_text(&self) -> &str {
        let (a, b) = (self.cursor.min(self.anchor), self.cursor.max(self.anchor));
        &self.text()[a..b]
    }

    // Seitenwechsel: der Cursor steht am Ende des Texts (setValue).
    fn load_page(&mut self) {
        self.cursor = self.text().len();
        self.anchor = self.cursor;
    }

    pub fn back(&mut self) -> bool {
        if self.page == 0 {
            return false;
        }
        self.page -= 1;
        self.load_page();
        true
    }

    // Weiter; auf der letzten Seite kommt (bis 100) eine leere dazu.
    pub fn forward(&mut self) -> bool {
        if self.page + 1 >= self.pages.len() && self.pages.len() < MAX_PAGES {
            self.pages.push(String::new());
        }
        if self.page + 1 >= self.pages.len() {
            return false;
        }
        self.page += 1;
        self.load_page();
        true
    }

    // Die Seiten fuer edit_book: leere am Ende fallen weg (eraseEmptyTrailingPages).
    pub fn finished_pages(&mut self) -> Vec<String> {
        while self.pages.last().is_some_and(|p| p.is_empty()) {
            self.pages.pop();
        }
        self.pages.clone()
    }

    // MultilineTextField.insertText: ersetzt die Markierung. Was nicht erlaubt ist, faellt
    // weg ('\n' bleibt), es wird auf die freien Zeichen gekuerzt, und was ueber 14 Zeilen
    // hinausliefe, wird gar nicht eingefuegt.
    pub fn insert(&mut self, s: &str, w: &dyn Fn(char) -> f32) {
        if s.is_empty() && self.selection().is_none() {
            return;
        }
        let gefiltert: String = s.chars().filter(|&c| allowed_char(c) || c == '\n').collect();
        let frei = PAGE_CHAR_LIMIT.saturating_sub(len16(self.text()));
        let einfuegen = truncate16(&gefiltert, frei);
        let (a, b) = (self.cursor.min(self.anchor), self.cursor.max(self.anchor));
        let neu = format!("{}{}{}", &self.text()[..a], einfuegen, &self.text()[b..]);
        if overflows_lines(&neu, w) {
            return;
        }
        self.pages[self.page] = neu;
        self.cursor = a + einfuegen.len();
        self.anchor = self.cursor;
    }

    // `zeichen` Zeichen vor (positiv) bzw. zurueck loeschen -- oder die Markierung.
    fn delete(&mut self, ziel: usize, w: &dyn Fn(char) -> f32) {
        if self.selection().is_none() {
            self.anchor = ziel;
        }
        self.insert("", w);
    }

    fn seek(&mut self, pos: usize) {
        self.cursor = pos.min(self.text().len());
        if !self.selecting {
            self.anchor = self.cursor;
        }
    }

    fn prev_char(&self, i: usize) -> usize {
        self.text()[..i].char_indices().next_back().map_or(0, |(j, _)| j)
    }

    fn next_char(&self, i: usize) -> usize {
        self.text()[i..].chars().next().map_or(i, |c| i + c.len_utf8())
    }

    fn line_at_cursor(&self, zeilen: &[(usize, usize)]) -> usize {
        zeilen.iter().position(|&(a, b)| a <= self.cursor && self.cursor <= b).unwrap_or(zeilen.len() - 1)
    }

    // Startpunkt der Wortsuche: der Cursor, hoechstens auf dem letzten Zeichen.
    fn word_start_point(&self) -> usize {
        let t = self.text();
        self.cursor.min(t.char_indices().next_back().map_or(0, |(j, _)| j))
    }

    fn word_end(&self, mut i: usize) -> usize {
        let t = self.text();
        while let Some(c) = t[i..].chars().next().filter(|c| !c.is_whitespace()) {
            i += c.len_utf8();
        }
        i
    }

    // Anfang und Ende des Worts vor dem Cursor (getPreviousWord).
    fn previous_word(&self) -> (usize, usize) {
        if self.text().is_empty() {
            return (0, 0);
        }
        let ws_davor = |i: usize| self.text()[..i].chars().next_back().is_some_and(char::is_whitespace);
        let mut i = self.word_start_point();
        while i > 0 && ws_davor(i) {
            i = self.prev_char(i);
        }
        while i > 0 && !ws_davor(i) {
            i = self.prev_char(i);
        }
        (i, self.word_end(i))
    }

    // Anfang und Ende des naechsten Worts (getNextWord).
    fn next_word(&self) -> (usize, usize) {
        let t = self.text();
        if t.is_empty() {
            return (0, 0);
        }
        let mut i = self.word_start_point();
        while let Some(c) = t[i..].chars().next().filter(|c| !c.is_whitespace()) {
            i += c.len_utf8();
        }
        while let Some(c) = t[i..].chars().next().filter(|c| c.is_whitespace()) {
            i += c.len_utf8();
        }
        (i, self.word_end(i))
    }

    // Eine Zeile hoch bzw. runter, moeglichst an derselben Stelle (seekCursorLine).
    fn seek_line(&mut self, schritt: isize, w: &dyn Fn(char) -> f32) {
        let zeilen = display_lines(self.text(), w);
        let jetzt = self.line_at_cursor(&zeilen);
        let x = width(&self.text()[zeilen[jetzt].0..self.cursor], w) + 2.0;
        let ziel = zeilen[(jetzt as isize + schritt).clamp(0, zeilen.len() as isize - 1) as usize];
        let pos = ziel.0 + head_by_width(&self.text()[ziel.0..ziel.1], x, w);
        self.seek(pos);
    }

    pub fn key(&mut self, key: EditKey, shift: bool, ctrl: bool, w: &dyn Fn(char) -> f32) {
        self.selecting = shift;
        match key {
            EditKey::Left if ctrl => self.seek(self.previous_word().0),
            EditKey::Left => self.seek(self.prev_char(self.cursor)),
            EditKey::Right if ctrl => self.seek(self.next_word().0),
            EditKey::Right => self.seek(self.next_char(self.cursor)),
            EditKey::Up | EditKey::Down if ctrl => {}
            EditKey::Up => self.seek_line(-1, w),
            EditKey::Down => self.seek_line(1, w),
            EditKey::Home if ctrl => self.seek(0),
            EditKey::End if ctrl => self.seek(self.text().len()),
            EditKey::Home | EditKey::End => {
                let zeilen = display_lines(self.text(), w);
                let (a, b) = zeilen[self.line_at_cursor(&zeilen)];
                self.seek(if key == EditKey::Home { a } else { b });
            }
            EditKey::Backspace => {
                let ziel = if ctrl { self.previous_word().0 } else { self.prev_char(self.cursor) };
                self.delete(ziel, w);
            }
            EditKey::Delete => {
                let ziel = if ctrl { self.next_word().0 } else { self.next_char(self.cursor) };
                self.delete(ziel, w);
            }
            EditKey::Enter => self.insert("\n", w),
        }
    }

    pub fn select_all(&mut self) {
        self.cursor = self.text().len();
        self.anchor = 0;
    }

    // Strg+X: den markierten Text herausnehmen.
    pub fn cut(&mut self, w: &dyn Fn(char) -> f32) -> String {
        let text = self.selected_text().to_string();
        self.insert("", w);
        text
    }

    // Klick ins Feld bei (x, y) GUI-Pixeln relativ zum Textanfang (seekCursorToPoint); mit
    // Shift wird die Markierung erweitert, ein Doppelklick markiert das Wort.
    pub fn click(&mut self, x: f32, y: f32, shift: bool, doppelt: bool, w: &dyn Fn(char) -> f32) {
        if doppelt {
            let (a, b) = self.previous_word();
            self.selecting = false;
            self.seek(a);
            self.selecting = true;
            self.seek(b);
            return;
        }
        self.selecting = shift;
        self.seek_to_point(x, y, w);
    }

    // Ziehen mit gedrueckter Taste markiert.
    pub fn drag(&mut self, x: f32, y: f32, w: &dyn Fn(char) -> f32) {
        self.selecting = true;
        self.seek_to_point(x, y, w);
        self.selecting = false;
    }

    fn seek_to_point(&mut self, x: f32, y: f32, w: &dyn Fn(char) -> f32) {
        let zeilen = display_lines(self.text(), w);
        let zeile = ((y / LINE_H).floor() as isize).clamp(0, zeilen.len() as isize - 1) as usize;
        let (a, b) = zeilen[zeile];
        let pos = a + head_by_width(&self.text()[a..b], x.floor(), w);
        self.seek(pos);
    }

    // Titelfeld (EditBox, hoechstens 15 Zeichen): Tippen haengt an, die Ruecktaste loescht.
    pub fn title_insert(&mut self, s: &str) {
        let gefiltert: String = s.chars().filter(|&c| allowed_char(c)).collect();
        let frei = TITLE_LIMIT.saturating_sub(len16(&self.title));
        self.title.push_str(truncate16(&gefiltert, frei));
    }

    pub fn title_backspace(&mut self) {
        self.title.pop();
    }

    // Signieren geht erst mit einem Titel, der nicht nur aus Leerzeichen besteht.
    pub fn can_sign(&self) -> bool {
        !self.title.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seiten_und_anzeige() {
        let mut stack = ItemStack::new(1, 1);
        assert_eq!(pages_of(&stack).len(), 1, "ohne Inhalt eine leere Seite");
        stack.writable_pages = Some(vec!["eins".into(), "zwei".into()]);
        let mut b = BookView::new(&stack);
        assert_eq!(b.pages.len(), 2);
        b.back();
        assert_eq!(b.page, 0);
        b.forward();
        b.forward();
        assert_eq!(b.page, 1, "nicht ueber die letzte Seite hinaus");
        assert_eq!(page_indicator(0, 3), "Page 1 of 3");
    }

    // Jedes Zeichen 6 Schriftpixel breit, das Leerzeichen 4 (wie in der Standardschrift).
    fn w(c: char) -> f32 {
        if c == ' ' {
            4.0
        } else {
            6.0
        }
    }

    #[test]
    fn umbruch_wie_stringsplitter() {
        assert_eq!(split_lines("", TEXT_W, &w), vec![]);
        assert_eq!(display_lines("", &w), vec![(0, 0)]);
        // 19 Zeichen passen (114), das 20. nicht: ohne Leerzeichen hart davor.
        let lang = "x".repeat(25);
        assert_eq!(split_lines(&lang, TEXT_W, &w), vec![(0, 19), (19, 25)]);
        // Mit Leerzeichen dort, das Leerzeichen gehoert zu keiner Zeile.
        let t = format!("{} {}", "a".repeat(10), "b".repeat(15));
        assert_eq!(split_lines(&t, TEXT_W, &w), vec![(0, 10), (11, 26)]);
        assert_eq!(display_lines("ab\n", &w), vec![(0, 2), (3, 3)]);
        assert_eq!(display_lines("ab\ncd", &w), vec![(0, 2), (3, 5)]);
    }

    #[test]
    fn tippen_loeschen_und_grenzen() {
        let mut b = BookEdit::new(40, Vec::new());
        b.insert("Hallo\u{a7}c Welt\t", &w);
        assert_eq!(b.text(), "Halloc Welt", "Paragraph und Tab fallen weg");
        b.key(EditKey::Backspace, false, true, &w);
        assert_eq!(b.text(), "Halloc ", "Strg+Ruecktaste loescht das Wort");
        b.key(EditKey::Home, false, false, &w);
        b.key(EditKey::Right, true, false, &w);
        b.key(EditKey::Right, true, false, &w);
        assert_eq!(b.selected_text(), "Ha");
        b.insert("X", &w);
        assert_eq!(b.text(), "Xlloc ");
        // 14 Zeilen und nicht mehr.
        let mut b = BookEdit::new(0, Vec::new());
        for _ in 0..20 {
            b.key(EditKey::Enter, false, false, &w);
        }
        assert_eq!(b.text().matches('\n').count(), EDIT_LINE_LIMIT - 1);
        // 1024 Zeichen je Seite (mit Umbruechen alle 19 passt das nicht in 14 Zeilen, also
        // mit schmalen Zeichen).
        let mut b = BookEdit::new(0, Vec::new());
        b.insert(&"i".repeat(2000), &|_| 0.5);
        assert_eq!(b.text().len(), PAGE_CHAR_LIMIT);
    }

    #[test]
    fn zeilen_und_woerter_springen() {
        let mut b = BookEdit::new(0, vec!["eins zwei\ndrei".into()]);
        assert_eq!(b.cursor(), 14, "Cursor am Ende");
        b.key(EditKey::Up, false, false, &w);
        assert_eq!(b.cursor(), 4, "x der Zeile ueber 'drei' (+2) landet nach 'eins'");
        b.key(EditKey::Left, false, true, &w);
        assert_eq!(b.cursor(), 0);
        b.key(EditKey::Right, false, true, &w);
        assert_eq!(b.cursor(), 5, "Anfang von 'zwei'");
        b.key(EditKey::End, false, false, &w);
        assert_eq!(b.cursor(), 9, "Zeilenende");
        b.click(13.0, 10.0, false, false, &w);
        assert_eq!(b.cursor(), 12, "zwei Zeichen in die zweite Zeile");
        b.click(0.0, 0.0, false, true, &w);
        assert_eq!(b.selected_text(), "drei", "Doppelklick markiert das Wort vor dem Cursor");
    }

    #[test]
    fn seiten_und_titel() {
        let mut b = BookEdit::new(0, vec!["a".into()]);
        assert!(!b.back());
        assert!(b.forward(), "auf der letzten Seite kommt eine dazu");
        assert_eq!(b.pages.len(), 2);
        b.insert("b", &w);
        assert!(b.forward());
        assert_eq!(b.finished_pages(), vec!["a".to_string(), "b".to_string()], "leere Seite am Ende faellt weg");
        let mut b = BookEdit::new(0, vec![String::new(); MAX_PAGES]);
        b.page = MAX_PAGES - 1;
        assert!(!b.forward(), "nicht ueber 100 Seiten");
        b.title_insert("Ein sehr langer Buchtitel");
        assert_eq!(b.title, "Ein sehr langer");
        assert!(b.can_sign());
        b.title = "   ".into();
        assert!(!b.can_sign());
    }
}
