// Die Chat-Eingabezeile: Textpuffer, Verlauf und Vorschlagsliste.
//
// Bewusst ein eigenes Modul mit reiner Logik, ohne GPU und ohne App: das Feld hat inzwischen
// genug Zustand (Puffer, Verlaufszeiger, Vorschlaege, welcher davon gewaehlt ist), dass sich
// Fehler darin nur schwer durch Ausprobieren finden lassen -- die letzten zwei Chat-Fehler
// waren beides Zustandsfehler. Hier ist alles davon direkt testbar.
//
// Das Zeichnen bleibt in main.rs (es braucht Textbreiten aus der GPU), aber WAS gezeichnet
// wird -- welcher Ausschnitt des Textes, welcher Vorschlag hervorgehoben ist -- entscheidet
// sich hier.

// Laengengrenze des Eingabefeldes. Spiegelt das serverseitige String(256)-Limit im
// ChatMessageC2SPacket (s. protocol::packets::chat_message_payload) -- laenger wuerde der
// Server ohnehin ablehnen oder abschneiden. Vanillas Feld hat dieselbe Grenze.
pub const MAX_LEN: usize = 256;
// So viele Vorschlaege werden gleichzeitig gezeigt. Vanilla zeigt 10.
pub const MAX_VISIBLE_SUGGESTIONS: usize = 10;
// So viele gesendete Nachrichten bleiben im Verlauf. Er wird gespeichert (chat_history.txt)
// und ist als Liste neben der Eingabezeile sichtbar -- wie die History einer Shell.
pub const MAX_HISTORY: usize = 1000;

// Wohin der Pfeil zeigt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
}

// Wohin der Schreibcursor springt (Pfeile links/rechts, mit Strg wortweise, Pos1/Ende).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
}

// Was eine Bearbeitung geloescht hat: wo es im ALTEN Text begann, der Text selbst und ob der
// Cursor daran stand (Ruecktaste, Entf, Tastaturmarkierung -- nicht bei einer Mausmarkierung,
// dort bleibt der Cursor, wo er war). Fuer das Herausfallen der Zeichen (s. main.rs,
// erased_pieces): aus dem Text allein ist die Stelle oft nicht eindeutig.
#[derive(Clone, Debug, PartialEq)]
pub struct Erased {
    pub start: usize,
    pub text: String,
    pub at_cursor: bool,
}

#[derive(Debug, Default)]
pub struct ChatInput {
    text: String,
    // Schreibcursor als Byte-Offset in `text` (immer auf einer Zeichengrenze). Tippen fuegt
    // hier ein, die Ruecktaste loescht davor, Entf dahinter.
    cursor: usize,
    // Anfang einer Markierung mit Shift+Pfeil: von hier bis zum Cursor ist markiert. None =
    // keine Tastatur-Markierung.
    anchor: Option<usize>,
    // Gesendete Nachrichten, aelteste zuerst.
    history: Vec<String>,
    // Wie weit man mit dem Pfeil nach oben im Verlauf zurueckgegangen ist. 0 = im eigenen
    // Entwurf, 1 = letzte gesendete Nachricht, usw.
    history_offset: usize,
    // Der Entwurf, in dem man war, als man anfing im Verlauf zu blaettern -- Vanilla holt
    // ihn beim Zurueckblaettern wieder hervor, statt ihn zu verlieren.
    draft: String,
    // Vorschlaege der letzten Server-Antwort plus der Bereich im Text, den sie ERSETZEN.
    suggestions: Vec<String>,
    suggestion_start: usize,
    suggestion_len: usize,
    selected: usize,
    // Transaktions-ID der letzten Anfrage. Antworten mit einer aelteren ID werden verworfen:
    // waehrend man tippt, sind mehrere Anfragen unterwegs, und die Antworten koennen in
    // beliebiger Reihenfolge eintreffen -- ohne diese Pruefung ueberschreibt eine langsame
    // alte Antwort die frische und man vervollstaendigt zu dem, was man vor drei Zeichen
    // getippt hat.
    pending_id: i32,
    next_id: i32,
    // Zuletzt Geloeschtes und wie viele Bearbeitungen seit dem letzten take_erased kamen.
    erased: Option<Erased>,
    edits: u32,
}

impl ChatInput {
    pub fn new(text: String) -> Self {
        Self { cursor: text.len(), text, next_id: 1, ..Default::default() }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    // Die Tastatur-Markierung als Byte-Bereich, None = keine (oder leer).
    pub fn keyboard_selection(&self) -> Option<std::ops::Range<usize>> {
        let a = self.anchor?;
        (a != self.cursor).then(|| a.min(self.cursor)..a.max(self.cursor))
    }

    // Alles markieren (Strg+A): Anker am Anfang, Cursor am Ende.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    // Cursor bewegen. `extend` (Shift): die Markierung vom bisherigen Cursor aus aufziehen
    // bzw. erweitern; sonst endet eine Markierung. Gibt zurueck, ob sich der Cursor bewegt hat.
    pub fn move_cursor(&mut self, m: Move, extend: bool) -> bool {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        let t = &self.text;
        let vor = |i: usize| t[..i].chars().next_back().map_or(i, |c| i - c.len_utf8());
        let nach = |i: usize| t[i..].chars().next().map_or(i, |c| i + c.len_utf8());
        let ist_leer = |i: usize, rueckwaerts: bool| {
            let c = if rueckwaerts { t[..i].chars().next_back() } else { t[i..].chars().next() };
            c.map(|c| c == ' ')
        };
        let ziel = match m {
            Move::Left => vor(self.cursor),
            Move::Right => nach(self.cursor),
            Move::Home => 0,
            Move::End => t.len(),
            // Wie Vanillas getWordSkipPosition: rueckwaerts erst Leerzeichen, dann das Wort;
            // vorwaerts erst das Wort, dann die Leerzeichen dahinter.
            Move::WordLeft => {
                let mut i = self.cursor;
                while ist_leer(i, true) == Some(true) {
                    i = vor(i);
                }
                while ist_leer(i, true) == Some(false) {
                    i = vor(i);
                }
                i
            }
            Move::WordRight => {
                let mut i = self.cursor;
                while ist_leer(i, false) == Some(false) {
                    i = nach(i);
                }
                while ist_leer(i, false) == Some(true) {
                    i = nach(i);
                }
                i
            }
        };
        let bewegt = ziel != self.cursor;
        self.cursor = ziel;
        bewegt
    }

    // Den Cursor an eine Stelle setzen (z.B. an den Rand einer Mausmarkierung).
    pub fn set_cursor(&mut self, i: usize) {
        let mut i = i.min(self.text.len());
        while !self.text.is_char_boundary(i) {
            i -= 1;
        }
        self.cursor = i;
        self.anchor = None;
    }

    // Der Text bis zum Cursor: nur dafuer fragt Vanilla Vorschlaege an (die Antwort ersetzt
    // dann einen Bereich darin).
    pub fn text_before_cursor(&self) -> &str {
        &self.text[..self.cursor]
    }

    pub fn suggestions(&self) -> &[String] {
        &self.suggestions
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn has_suggestions(&self) -> bool {
        !self.suggestions.is_empty()
    }

    // Darf fuer den aktuellen Text ueberhaupt eine Vervollstaendigungs-Anfrage rausgehen?
    //
    // Vanilla schickt NIE einfach den Feldinhalt. Die Anfrage entsteht erst tief drin:
    // CommandSuggestions.updateCommandInfo laesst Brigadier den Text gegen den Befehlsbaum
    // parsen, den der Server beim Beitritt angekuendigt hat, und erst wenn der Durchlauf an
    // einem Argumentknoten mit ASK_SERVER ankommt, ruft der Knoten
    // ClientSuggestionProvider.customSuggestion auf -- das ist die einzige Stelle, die
    // ServerboundCommandSuggestionPacket sendet. Ein halb getippter Befehlsname kommt dort
    // nie an, weil er noch auf einen Literalknoten faellt, der seine Vorschlaege selbst hat.
    //
    // Wir haben den Befehlsbaum nicht, bilden aber die beiden beobachtbaren Folgen nach.
    // Ohne sie flaggt GrimAC sofort:
    //   ChatA  (grim.exploit.blank_tab_complete): Text ist genau "/" oder nur Leerraum.
    //   CrashH (grim.crash.invalid_tab_complete): laenger als 256 Zeichen, oder laenger als
    //          64 Zeichen ohne ein Leerzeichen in den ersten 64.
    // Beide Laengen zaehlt Grim in Java-`String`-Einheiten, also UTF-16, nicht in Bytes.
    pub fn wants_suggestions(&self) -> bool {
        let text = self.text_before_cursor();
        let Some(befehl) = text.strip_prefix('/') else { return false };
        // "/" allein und "/   ": in Vanilla unmoeglich, weil der leere Befehl nie an einem
        // Argumentknoten ankommt.
        if befehl.trim().is_empty() {
            return false;
        }
        // Erst ein abgeschlossener Befehlsname (Literal + Leerzeichen) kann ueberhaupt zu
        // einem Argumentknoten fuehren. "/spa" fragt in Vanilla nicht nach, "/tp " schon.
        // Ein Leerzeichen direkt hinter dem Schraegstrich ist kein Name.
        let Some(leerzeichen) = text.find(' ') else { return false };
        if leerzeichen <= 1 {
            return false;
        }
        let laenge: usize = text.chars().map(char::len_utf16).sum();
        if laenge > 256 {
            return false;
        }
        let leerzeichen_index: usize = text[..leerzeichen].chars().map(char::len_utf16).sum();
        if laenge > 64 && leerzeichen_index >= 64 {
            return false;
        }
        true
    }

    // Zeichen am Cursor einfuegen. Gibt zurueck, ob sich der Text geaendert hat -- der
    // Aufrufer schickt dann eine neue Vervollstaendigungs-Anfrage.
    pub fn push_str(&mut self, text: &str) -> bool {
        let vorher = self.text.len();
        for ch in text.chars() {
            if !ch.is_control() && self.text.chars().count() < MAX_LEN {
                self.text.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
            }
        }
        let geaendert = self.text.len() != vorher;
        if geaendert {
            self.on_edit();
        }
        geaendert
    }

    // Einen Bereich (Byte-Offsets in text()) durch `ersatz` ersetzen -- markierten Text
    // ueberschreiben oder (mit leerem Ersatz) loeschen. Dieselben Regeln wie push_str:
    // keine Steuerzeichen, hoechstens MAX_LEN Zeichen. Der Cursor bleibt, wo er im Text stand
    // -- bewegen soll ihn nur ein Klick (Wunsch des Nutzers); nur wenn er am Bereich oder darin
    // stand (Tastaturmarkierung), steht er danach hinter dem Neuen.
    pub fn replace_range(&mut self, bereich: std::ops::Range<usize>, ersatz: &str) -> bool {
        let ende = bereich.end.min(self.text.len());
        let start = bereich.start.min(ende);
        if !self.text.is_char_boundary(start) || !self.text.is_char_boundary(ende) {
            return false;
        }
        let rest = self.text.chars().count() - self.text[start..ende].chars().count();
        let neu: String = ersatz.chars().filter(|c| !c.is_control()).take(MAX_LEN.saturating_sub(rest)).collect();
        if start == ende && neu.is_empty() {
            return false;
        }
        let am_cursor = (start..=ende).contains(&self.cursor);
        self.erased = (start < ende).then(|| Erased { start, text: self.text[start..ende].to_string(), at_cursor: am_cursor });
        self.text.replace_range(start..ende, &neu);
        self.cursor = if self.cursor < start {
            self.cursor
        } else if self.cursor <= ende {
            start + neu.len()
        } else {
            self.cursor - (ende - start) + neu.len()
        };
        self.on_edit();
        true
    }

    // Ruecktaste: das Zeichen vor dem Cursor.
    pub fn backspace(&mut self) -> bool {
        let Some(c) = self.text[..self.cursor].chars().next_back() else { return false };
        self.cursor -= c.len_utf8();
        self.erased = Some(Erased { start: self.cursor, text: c.to_string(), at_cursor: true });
        self.text.remove(self.cursor);
        self.on_edit();
        true
    }

    // Entf: das Zeichen hinter dem Cursor.
    pub fn delete_forward(&mut self) -> bool {
        if self.cursor >= self.text.len() {
            return false;
        }
        let c = self.text.remove(self.cursor);
        self.erased = Some(Erased { start: self.cursor, text: c.to_string(), at_cursor: true });
        self.on_edit();
        true
    }

    // Nach jeder Aenderung von Hand: die Vorschlaege gelten fuer den alten Text und sind
    // damit hinfaellig, und man ist wieder in seinem eigenen Entwurf (nicht mehr im Verlauf).
    fn on_edit(&mut self) {
        self.edits += 1;
        self.anchor = None;
        self.suggestions.clear();
        self.selected = 0;
        self.history_offset = 0;
        self.draft.clear();
    }

    // Das seit dem letzten Aufruf Geloeschte. None, wenn nichts geloescht wurde oder mehr als
    // eine Bearbeitung dazukam -- dann passt die Stelle nicht mehr zum Text.
    pub fn take_erased(&mut self) -> Option<Erased> {
        let erased = self.erased.take();
        (std::mem::take(&mut self.edits) == 1).then_some(erased).flatten()
    }

    // Die naechste Anfrage-ID. Der Aufrufer schickt sie mit; nur die Antwort darauf gilt.
    pub fn begin_request(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.pending_id = id;
        id
    }

    // Antwort des Servers uebernehmen. Veraltete Antworten werden verworfen (s. pending_id).
    //
    // `start`/`length` sind Offsets in BYTES im angefragten Text (so kommen sie vom Server).
    // Sie werden hier gegen die Textlaenge geklemmt und auf eine Zeichengrenze geprueft --
    // ein Server, der Unsinn schickt, darf kein String-Slicing mitten in einem Mehrbyte-
    // Zeichen ausloesen (das waere eine Panik, nicht nur eine falsche Anzeige).
    pub fn apply_suggestions(&mut self, id: i32, start: i32, length: i32, matches: Vec<String>) -> bool {
        if id != self.pending_id {
            return false;
        }
        let start = start.max(0) as usize;
        let length = length.max(0) as usize;
        if start > self.text.len() || start + length > self.text.len() {
            return false;
        }
        if !self.text.is_char_boundary(start) || !self.text.is_char_boundary(start + length) {
            return false;
        }
        self.suggestion_start = start;
        self.suggestion_len = length;
        self.suggestions = matches;
        self.selected = 0;
        true
    }

    pub fn clear_suggestions(&mut self) {
        self.suggestions.clear();
        self.selected = 0;
    }

    // Den gewaehlten Vorschlag einsetzen. Ersetzt den vom Server benannten Bereich, haengt
    // ihn NICHT an: bei "/tp Der" schickt der Server start=4, length=3 und den vollen Namen
    // "DerAntonia" -- angehaengt kaeme "/tp DerDerAntonia" heraus.
    pub fn apply_selected(&mut self) -> bool {
        let Some(suggestion) = self.suggestions.get(self.selected).cloned() else {
            return false;
        };
        let mut neu = String::with_capacity(self.text.len() + suggestion.len());
        neu.push_str(&self.text[..self.suggestion_start]);
        neu.push_str(&suggestion);
        neu.push_str(&self.text[self.suggestion_start + self.suggestion_len..]);
        // Nach dem Einsetzen die Laengengrenze einhalten, sonst koennte ein langer
        // Servervorschlag ein Feld erzeugen, das sich nicht mehr absenden laesst.
        if neu.chars().count() > MAX_LEN {
            return false;
        }
        self.text = neu;
        self.cursor = self.suggestion_start + suggestion.len();
        self.anchor = None;
        self.suggestions.clear();
        self.selected = 0;
        true
    }

    // Pfeil oben/unten. Sind Vorschlaege offen, blaettern sie DURCH DIE LISTE; sonst durch
    // den Verlauf. Genau diese Doppelbelegung hat Vanilla auch -- und sie ist der Grund,
    // warum beides in einem Modul steckt: getrennt waere leicht zu uebersehen, dass sich die
    // zwei Bedeutungen an derselben Taste treffen.
    pub fn step(&mut self, step: Step) {
        if self.has_suggestions() {
            let n = self.suggestions.len();
            self.selected = match step {
                // Oben = in der Liste nach oben, mit Umlauf wie in Vanilla.
                Step::Up => (self.selected + n - 1) % n,
                Step::Down => (self.selected + 1) % n,
            };
            return;
        }
        self.step_history(step);
    }

    fn step_history(&mut self, step: Step) {
        if self.history.is_empty() {
            return;
        }
        let ziel = match step {
            Step::Up => (self.history_offset + 1).min(self.history.len()),
            Step::Down => self.history_offset.saturating_sub(1),
        };
        if ziel == self.history_offset {
            return;
        }
        // Beim ersten Schritt aus dem Entwurf heraus den Entwurf sichern.
        if self.history_offset == 0 {
            self.draft = self.text.clone();
        }
        self.history_offset = ziel;
        self.text = if ziel == 0 { self.draft.clone() } else { self.history[self.history.len() - ziel].clone() };
        self.cursor = self.text.len();
        self.anchor = None;
        // Vorschlaege gelten fuer den vorherigen Text.
        self.suggestions.clear();
        self.selected = 0;
    }

    // Absenden: gibt den Text zurueck (leer = nichts senden) und legt ihn in den Verlauf.
    pub fn take_for_send(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.anchor = None;
        if !text.is_empty() {
            // Dieselbe Nachricht nicht zweimal hintereinander im Verlauf -- sonst muss man
            // beim Wiederholen zweimal nach oben druecken (Vanilla verhaelt sich genauso).
            if self.history.last().map(String::as_str) != Some(text.as_str()) {
                self.history.push(text.clone());
                if self.history.len() > MAX_HISTORY {
                    self.history.remove(0);
                }
            }
        }
        text
    }

    // Der Verlauf ueberlebt das Schliessen des Chats -- man will die letzte Nachricht auch
    // nach dem Zumachen noch heraufholen koennen.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    pub fn restore_history(&mut self, history: Vec<String>) {
        self.history = history;
    }

    // Text ersetzen (Klick auf einen Verlaufseintrag). Wie das Blaettern mit dem Pfeil: die
    // Vorschlaege gehoeren zum alten Text, der Verlaufszeiger beginnt wieder beim Entwurf.
    pub fn set_text(&mut self, text: &str) {
        self.text = text.chars().take(MAX_LEN).collect();
        self.cursor = self.text.len();
        self.anchor = None;
        self.history_offset = 0;
        self.suggestions.clear();
        self.selected = 0;
    }
}

// Der Text in Zeilen von hoechstens `max_width` Breite, von vorn gefuellt -- das
// mehrzeilige Eingabefeld: ist eine Zeile voll, geht es darunter weiter. Nie leer (ein leeres
// Feld ist eine leere Zeile). Umbrochen wird an Zeichengrenzen, bevorzugt nach einem
// Leerzeichen, wenn eines in der Zeile steht.
pub fn wrap_input_lines(text: &str, max_width: f32, width_of: impl Fn(&str) -> f32) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() && width_of(rest) > max_width {
        // Laengster Anfang, der passt (mindestens ein Zeichen, sonst kaeme man nie weiter).
        let mut ende = rest.char_indices().nth(1).map_or(rest.len(), |(i, _)| i);
        for (i, ch) in rest.char_indices().skip(1) {
            if width_of(&rest[..i + ch.len_utf8()]) > max_width {
                break;
            }
            ende = i + ch.len_utf8();
        }
        // Nach dem letzten Leerzeichen trennen, wenn das nicht die ganze Zeile verschenkt.
        if let Some(leer) = rest[..ende].rfind(' ').filter(|&i| i > 0 && ende < rest.len()) {
            ende = leer + 1;
        }
        lines.push(&rest[..ende]);
        rest = &rest[ende..];
    }
    lines.push(rest);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_wandert_und_bearbeitet_an_seiner_stelle() {
        let mut feld = ChatInput::new(String::new());
        feld.push_str("hallo welt");
        assert_eq!(feld.cursor(), 10);
        feld.move_cursor(Move::WordLeft, false);
        assert_eq!(feld.cursor(), 6);
        feld.push_str("ä");
        assert_eq!(feld.text(), "hallo äwelt");
        assert!(feld.backspace());
        feld.move_cursor(Move::Left, false);
        assert!(feld.delete_forward(), "Entf loescht das Leerzeichen hinter dem Cursor");
        assert_eq!(feld.text(), "hallowelt");
        feld.move_cursor(Move::Home, false);
        assert!(!feld.backspace(), "am Anfang hat die Ruecktaste nichts vor sich");
        feld.move_cursor(Move::WordRight, false);
        assert_eq!(feld.cursor(), 9);
        assert!(!feld.move_cursor(Move::Right, false), "am Ende bleibt er stehen");
        // Ueber Umlaute zeichenweise, nie mitten ins Zeichen.
        let mut u = ChatInput::new("aä b".into());
        u.move_cursor(Move::Left, false);
        u.move_cursor(Move::Left, false);
        u.move_cursor(Move::Left, false);
        assert_eq!(u.cursor(), 1);
        u.move_cursor(Move::WordRight, false);
        assert_eq!(u.cursor(), 4, "Wort und Leerzeichen dahinter: {:?}", u.text());
    }

    #[test]
    fn shift_markiert_vom_alten_cursor_aus() {
        let mut feld = ChatInput::new("eins zwei".into());
        feld.move_cursor(Move::WordLeft, true);
        assert_eq!(feld.keyboard_selection(), Some(5..9));
        feld.move_cursor(Move::Left, true);
        assert_eq!(feld.keyboard_selection(), Some(4..9));
        feld.move_cursor(Move::Right, false);
        assert_eq!(feld.keyboard_selection(), None, "ohne Shift endet die Markierung");
        assert_eq!(feld.cursor(), 5);
        assert!(feld.replace_range(0..4, "drei!"));
        assert_eq!(feld.cursor(), 6, "der Cursor stand hinter dem Bereich und bleibt dort");
        feld.set_cursor(0);
        assert!(feld.replace_range(0..5, "x"));
        assert_eq!(feld.cursor(), 1, "am Bereich: danach hinter dem Neuen");
    }

    #[test]
    fn vorschlaege_gelten_bis_zum_cursor() {
        let mut feld = ChatInput::new("/tp Der weiter".into());
        assert!(feld.wants_suggestions());
        feld.set_cursor(3);
        assert!(!feld.wants_suggestions(), "\"/tp\" ohne Leerzeichen fragt nicht nach");
        feld.set_cursor(7);
        let id = feld.begin_request();
        assert!(feld.apply_suggestions(id, 4, 3, vec!["DerAntonia".into()]));
        assert!(feld.apply_selected());
        assert_eq!(feld.text(), "/tp DerAntonia weiter");
        assert_eq!(feld.cursor(), 14);
    }

    #[test]
    fn markierter_bereich_wird_ersetzt_und_geloescht() {
        let mut feld = ChatInput::new(String::new());
        feld.push_str("hallo schoene welt");
        assert!(feld.replace_range(6..13, "grosse"));
        assert_eq!(feld.text(), "hallo grosse welt");
        assert_eq!(feld.cursor(), feld.text().len(), "Cursor am Ende bleibt am Ende");
        feld.set_cursor(2);
        assert!(feld.replace_range(6..13, ""));
        assert_eq!((feld.text(), feld.cursor()), ("hallo welt", 2), "Cursor davor bleibt stehen");
        assert!(feld.replace_range(0..feld.text().len(), ""), "alles markiert + Löschen");
        assert_eq!(feld.text(), "");
        assert!(!feld.replace_range(0..0, ""), "nichts markiert, nichts getippt: keine Änderung");
    }

    // Ein Zeichen = eine Breiteneinheit; damit sind die Tests unabhaengig von der echten
    // Schrift und pruefen die Logik statt der Metrik.
    fn breite(s: &str) -> f32 {
        s.chars().count() as f32
    }

    #[test]
    fn eingabe_bricht_in_zeilen_um() {
        assert_eq!(wrap_input_lines("", 5.0, breite), [""]);
        assert_eq!(wrap_input_lines("kurz", 5.0, breite), ["kurz"]);
        assert_eq!(wrap_input_lines("abcdefghij", 4.0, breite), ["abcd", "efgh", "ij"]);
        // Bevorzugt nach dem Leerzeichen, der Text bleibt dabei vollstaendig.
        assert_eq!(wrap_input_lines("ab cdef gh", 5.0, breite), ["ab ", "cdef ", "gh"]);
        assert_eq!(wrap_input_lines("ab cdef gh", 5.0, breite).concat(), "ab cdef gh");
        // Umlaute: an Zeichengrenzen.
        assert_eq!(wrap_input_lines("äöüß", 2.0, breite), ["äö", "üß"]);
    }

    #[test]
    fn verlaufseintrag_ersetzt_den_text() {
        let mut chat = ChatInput::new("alt".into());
        chat.set_text("neu");
        assert_eq!(chat.text(), "neu");
    }

    #[test]
    fn tippen_und_loeschen() {
        let mut chat = ChatInput::new(String::new());
        assert!(chat.push_str("hallo"));
        assert_eq!(chat.text(), "hallo");
        assert!(chat.backspace());
        assert_eq!(chat.text(), "hall");
        // Steuerzeichen werden nicht uebernommen, und ein wirkungsloser Aufruf meldet false
        // (der Aufrufer soll dafuer keine Vervollstaendigung anfragen).
        assert!(!chat.push_str("\n"));
        let mut leer = ChatInput::new(String::new());
        assert!(!leer.backspace());
    }

    #[test]
    fn laengengrenze_wird_eingehalten() {
        let mut chat = ChatInput::new(String::new());
        chat.push_str(&"x".repeat(MAX_LEN + 50));
        assert_eq!(chat.text().chars().count(), MAX_LEN);
    }

    #[test]
    // Verlauf: oben holt die letzte Nachricht, weiter oben die davor, unten wieder zurueck --
    // und ganz unten steht der eigene Entwurf, der beim Blaettern nicht verloren geht.
    fn verlauf_blaettert_und_bewahrt_den_entwurf() {
        let mut chat = ChatInput::new(String::new());
        chat.push_str("erste");
        chat.take_for_send();
        chat.push_str("zweite");
        chat.take_for_send();
        chat.push_str("entwurf");

        chat.step(Step::Up);
        assert_eq!(chat.text(), "zweite", "oben holt die NEUESTE Nachricht");
        chat.step(Step::Up);
        assert_eq!(chat.text(), "erste");
        chat.step(Step::Up);
        assert_eq!(chat.text(), "erste", "am Ende des Verlaufs bleibt es stehen");
        chat.step(Step::Down);
        assert_eq!(chat.text(), "zweite");
        chat.step(Step::Down);
        assert_eq!(chat.text(), "entwurf", "der Entwurf kommt zurück");
        chat.step(Step::Down);
        assert_eq!(chat.text(), "entwurf", "und bleibt");
    }

    #[test]
    // Zweimal dasselbe abschicken darf nicht zwei Verlaufseintraege ergeben -- sonst muesste
    // man zweimal nach oben druecken, um dieselbe Nachricht zu wiederholen.
    fn gleiche_nachricht_kommt_nur_einmal_in_den_verlauf() {
        let mut chat = ChatInput::new(String::new());
        for _ in 0..3 {
            chat.push_str("gg");
            chat.take_for_send();
        }
        assert_eq!(chat.history(), ["gg"]);
    }

    #[test]
    fn leere_nachricht_landet_nicht_im_verlauf() {
        let mut chat = ChatInput::new(String::new());
        assert_eq!(chat.take_for_send(), "");
        assert!(chat.history().is_empty());
    }

    #[test]
    // Der wichtigste Punkt der Vervollstaendigung: der Vorschlag ERSETZT den vom Server
    // benannten Bereich. Haengte man ihn an, kaeme "/tp DerDerAntonia" heraus -- der
    // haeufigste Fehler bei dieser Schnittstelle.
    fn vorschlag_ersetzt_statt_anzuhaengen() {
        let mut chat = ChatInput::new(String::new());
        chat.push_str("/tp Der");
        let id = chat.begin_request();
        assert!(chat.apply_suggestions(id, 4, 3, vec!["DerAntonia".into(), "Derp".into()]));
        assert!(chat.apply_selected());
        assert_eq!(chat.text(), "/tp DerAntonia");
    }

    #[test]
    // Ein Vorschlag mitten im Text (weitere Argumente dahinter) darf den Rest nicht fressen.
    fn vorschlag_in_der_mitte_laesst_den_rest_stehen() {
        let mut chat = ChatInput::new(String::new());
        chat.push_str("/give Der stone");
        let id = chat.begin_request();
        // Bereich "Der" ersetzen, " stone" bleibt.
        assert!(chat.apply_suggestions(id, 6, 3, vec!["DerAntonia".into()]));
        assert!(chat.apply_selected());
        assert_eq!(chat.text(), "/give DerAntonia stone");
    }

    #[test]
    // Antworten koennen in falscher Reihenfolge eintreffen. Nur die auf die JUENGSTE Anfrage
    // darf gelten -- sonst vervollstaendigt man zu dem, was drei Zeichen vorher galt.
    fn veraltete_antwort_wird_verworfen() {
        let mut chat = ChatInput::new(String::new());
        chat.push_str("/tp Der");
        let alt = chat.begin_request();
        let neu = chat.begin_request();
        assert!(!chat.apply_suggestions(alt, 4, 3, vec!["Falsch".into()]), "alte Antwort muss abgelehnt werden");
        assert!(chat.suggestions().is_empty());
        assert!(chat.apply_suggestions(neu, 4, 3, vec!["Richtig".into()]));
        assert_eq!(chat.suggestions(), ["Richtig"]);
    }

    #[test]
    // Unsinnige Bereiche vom Server: ablehnen statt panisch slicen. Ein Offset mitten in
    // einem Mehrbyte-Zeichen ist der gefaehrliche Fall.
    fn unsinnige_bereiche_werden_abgelehnt() {
        let mut chat = ChatInput::new(String::new());
        chat.push_str("/tp Ü");
        let id = chat.begin_request();
        assert!(!chat.apply_suggestions(id, 999, 1, vec!["x".into()]), "Start hinter dem Text");
        let id = chat.begin_request();
        assert!(!chat.apply_suggestions(id, 4, 999, vec!["x".into()]), "Länge über das Ende hinaus");
        let id = chat.begin_request();
        // "Ü" beginnt bei Byte 4 und ist zwei Bytes lang -- 5 ist keine Zeichengrenze.
        assert!(!chat.apply_suggestions(id, 5, 1, vec!["x".into()]), "Offset mitten im Zeichen");
        let id = chat.begin_request();
        assert!(chat.apply_suggestions(id, 4, 2, vec!["Uwe".into()]), "gültige Grenze muss durchgehen");
        assert!(chat.apply_selected());
        assert_eq!(chat.text(), "/tp Uwe");
    }

    #[test]
    // Bei offener Vorschlagsliste blaettern die Pfeile DURCH DIE LISTE, nicht durch den
    // Verlauf -- sonst waere die Liste unbedienbar.
    fn pfeile_bedienen_die_liste_wenn_sie_offen_ist() {
        let mut chat = ChatInput::new(String::new());
        chat.push_str("erste");
        chat.take_for_send();
        chat.push_str("/tp ");
        let id = chat.begin_request();
        chat.apply_suggestions(id, 4, 0, vec!["a".into(), "b".into(), "c".into()]);
        chat.step(Step::Down);
        assert_eq!(chat.selected(), 1);
        assert_eq!(chat.text(), "/tp ", "der Text darf sich dabei NICHT ändern");
        chat.step(Step::Up);
        assert_eq!(chat.selected(), 0);
        // Umlauf nach oben, wie in Vanilla.
        chat.step(Step::Up);
        assert_eq!(chat.selected(), 2);
        // Erst ohne Liste greift der Verlauf.
        chat.clear_suggestions();
        chat.step(Step::Up);
        assert_eq!(chat.text(), "erste");
    }

    #[test]
    // Tippen macht die Vorschlaege hinfaellig: sie gelten fuer den Text von vorher, und ihr
    // Ersetzungsbereich wuerde nach einem weiteren Zeichen ins Leere zeigen.
    fn tippen_verwirft_die_vorschlaege() {
        let mut chat = ChatInput::new(String::new());
        chat.push_str("/tp Der");
        let id = chat.begin_request();
        chat.apply_suggestions(id, 4, 3, vec!["DerAntonia".into()]);
        assert!(chat.has_suggestions());
        chat.push_str("A");
        assert!(!chat.has_suggestions());
        // Und ohne Vorschlaege tut das Einsetzen nichts (statt zu panicken).
        assert!(!chat.apply_selected());
    }

    // Die Faelle, die GrimAC als unmoegliches Paket ansieht, duerfen gar nicht erst
    // rausgehen. Werte aus Grims eigenen Checks (ChatA, CrashH) und aus Vanillas
    // CommandSuggestions/ClientSuggestionProvider.
    #[test]
    fn nur_vanilla_moegliche_vervollstaendigungen_gehen_raus() {
        let frage = |t: &str| ChatInput::new(t.to_string()).wants_suggestions();
        assert!(!frage(""), "leeres Feld ist kein Befehl");
        assert!(!frage("hallo welt"), "normaler Text fragt nie nach");
        assert!(!frage("/"), "ChatA: der nackte Schrägstrich");
        assert!(!frage("/   "), "ChatA: nur Leerraum hinter dem Schrägstrich");
        assert!(!frage("/ tp"), "Leerzeichen direkt hinter dem Schrägstrich ist kein Name");
        assert!(!frage("/tp"), "halb getippter Name landet in Vanilla auf einem Literalknoten");
        assert!(frage("/tp "), "abgeschlossener Name: jetzt kommt ein Argumentknoten");
        assert!(frage("/tp DerAntonia"), "Argument wird vervollständigt");
    }

    #[test]
    fn zu_lange_vervollstaendigungen_gehen_nicht_raus() {
        let lang = format!("/msg {}", "a".repeat(300));
        assert!(!ChatInput::new(lang).wants_suggestions(), "CrashH: länger als 256 Zeichen");

        // CrashH zweite Haelfte: ueber 64 Zeichen und das erste Leerzeichen liegt nicht mehr
        // in den ersten 64.
        let spaeter = format!("/{} x", "a".repeat(70));
        assert!(!ChatInput::new(spaeter).wants_suggestions());
        let frueh = format!("/msg {}", "a".repeat(70));
        assert!(ChatInput::new(frueh).wants_suggestions(), "frühes Leerzeichen ist harmlos");

        // Grim zaehlt UTF-16, nicht Bytes: 100 Umlaute sind 200 Bytes, aber nur 100 Zeichen.
        let umlaute = format!("/msg {}", "ä".repeat(100));
        assert!(ChatInput::new(umlaute).wants_suggestions());
    }

    #[test]
    // Blaettern im Verlauf setzt den Zeiger zurueck, sobald man wieder selbst tippt --
    // sonst wuerde der naechste Pfeil-oben von der falschen Stelle weiterlaufen.
    fn tippen_verlaesst_den_verlauf() {
        let mut chat = ChatInput::new(String::new());
        chat.push_str("alt");
        chat.take_for_send();
        chat.step(Step::Up);
        assert_eq!(chat.text(), "alt");
        chat.push_str("x");
        assert_eq!(chat.text(), "altx");
        chat.step(Step::Up);
        assert_eq!(chat.text(), "alt", "oben holt wieder die letzte Nachricht");
    }

    #[test]
    fn geloeschtes_wird_mit_stelle_gemeldet() {
        let mut chat = ChatInput::new("aasdasdasd".into());
        chat.backspace();
        assert_eq!(chat.take_erased(), Some(Erased { start: 9, text: "d".into(), at_cursor: true }));
        assert_eq!(chat.take_erased(), None, "nur einmal");
        // Mausmarkierung weit vom Cursor (der am Ende steht).
        chat.replace_range(1..4, "");
        assert_eq!(chat.take_erased(), Some(Erased { start: 1, text: "asd".into(), at_cursor: false }));
        // Zwei Bearbeitungen vor dem Abholen: die Stelle passt nicht mehr, keine Meldung.
        chat.backspace();
        chat.push_str("x");
        assert_eq!(chat.take_erased(), None);
    }
}
