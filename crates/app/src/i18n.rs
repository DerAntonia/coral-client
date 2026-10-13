// Sprachen des Clients (Einstellungen -> Sprache). Zwei getrennte Dinge:
//
// - Die OBERFLAECHE dieses Clients ist auf Deutsch geschrieben. Uebersetzt wird sie erst beim
//   Messen und Zeichnen (s. GpuState::measure_text/shape_text): jeder Text, der woertlich in
//   i18n_ui.tsv steht, kommt in der gewaehlten Sprache heraus. So muss keine der vielen
//   Stellen, die Beschriftungen bauen, davon wissen -- und weil Messen und Zeichnen dieselbe
//   Uebersetzung sehen, stimmen Zentrierung und Breiten weiter.
// - Texte des SPIELS (Itemnamen, Effekte, Chat) kommen aus Minecrafts Sprachdateien (de_de,
//   en_us, la_la), je Bereich eine eigene (s. protocol::nbt::LangArea).

use std::borrow::Cow;
use rustc_hash::FxHashMap as HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    German,
    English,
    Latin,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::German, Language::English, Language::Latin];

    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn from_index(i: u8) -> Self {
        Self::ALL.get(i as usize).copied().unwrap_or(Language::German)
    }

    pub fn next(self) -> Self {
        Self::from_index((self.index() + 1) % Self::ALL.len() as u8)
    }

    // In der Sprache selbst, wie Vanillas Sprachauswahl -- wer die Oberflaeche nicht lesen
    // kann, findet so trotzdem zurueck.
    pub fn label(self) -> &'static str {
        match self {
            Language::German => "Deutsch",
            Language::English => "English",
            Language::Latin => "Latīna",
        }
    }

    // Minecrafts Sprachdatei (assets/minecraft/lang/<code>.json) und die Locale fuer den Server.
    pub fn code(self) -> &'static str {
        match self {
            Language::German => "de_de",
            Language::English => "en_us",
            Language::Latin => "la_la",
        }
    }
}

static UI_LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub fn set_ui_language(lang: Language) {
    UI_LANGUAGE.store(lang.index(), Ordering::Relaxed);
}

pub fn ui_language() -> Language {
    Language::from_index(UI_LANGUAGE.load(Ordering::Relaxed))
}

// Latein mit Laengezeichen? Die Tabelle traegt sie; AUS nimmt sie beim Uebersetzen weg.
static LATIN_MACRONS: AtomicBool = AtomicBool::new(true);

pub fn set_latin_macrons(an: bool) {
    LATIN_MACRONS.store(an, Ordering::Relaxed);
}

// ā -> a usw.; alles andere bleibt.
pub fn strip_macrons(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ā' => 'a',
            'ē' => 'e',
            'ī' => 'i',
            'ō' => 'o',
            'ū' => 'u',
            'ȳ' => 'y',
            'Ā' => 'A',
            'Ē' => 'E',
            'Ī' => 'I',
            'Ō' => 'O',
            'Ū' => 'U',
            c => c,
        })
        .collect()
}

// Deutsch mit englischen Lehnwoertern? Spielbegriffe stehen dann so, wie man sie beim Spielen
// sagt (s. LOANWORDS). Gilt nur fuer Texte aus der Tabelle -- Chat, Itemnamen und
// Server-Texte bleiben, wie sie sind.
static GERMAN_LOANWORDS: AtomicBool = AtomicBool::new(false);

pub fn set_english_loanwords(an: bool) {
    GERMAN_LOANWORDS.store(an, Ordering::Relaxed);
}

// Deutscher Begriff -> englisches Lehnwort, der Reihe nach ersetzt (Laengeres vor dem
// Kuerzeren, das darin steckt). "Schild" allein fehlt mit Absicht: es heisst auch das Schild
// zum Beschriften.
const LOANWORDS: [(&str, &str); 20] = [
    ("Fadenkreuz", "Crosshair"),
    ("Zweithand", "Offhand"),
    ("Haupthand", "Mainhand"),
    ("Sichtfeld", "FOV"),
    ("Trefferbox", "Hitbox"),
    ("Abklingzeit", "Cooldown"),
    ("Bildrate", "FPS"),
    ("Schleichen", "Sneaken"),
    ("Zuschauer", "Spectator"),
    ("Tastenbelegung", "Keybinds"),
    ("Stapel", "Stack"),
    ("Kristalle", "Crystals"),
    ("Kristall", "Crystal"),
    ("Anker", "Anchor"),
    ("Spielmodus", "Gamemode"),
    ("Verzauberungs-Schimmer", "Glint"),
    ("Schilde", "Shields"),
    ("Schild-", "Shield-"),
    ("Munitionsanzeige", "Ammo-Anzeige"),
    ("Sprachchat", "Voicechat"),
];

// Erklaert den Schalter und nennt dafuer die deutschen Woerter selbst -- die bleiben.
pub const LOANWORDS_TOOLTIP: &str = "Nur für die deutsche Oberfläche: Spielbegriffe stehen auf Englisch, so wie man sie beim Spielen sagt – Crosshair statt Fadenkreuz, Offhand statt Zweithand, FOV statt Sichtfeld, Cooldown statt Abklingzeit. Chat und Itemnamen bleiben, wie sie sind.";

fn with_loanwords(text: &str) -> String {
    if text == LOANWORDS_TOOLTIP {
        return text.to_string();
    }
    LOANWORDS.iter().fold(text.to_string(), |t, (de, en)| t.replace(de, en))
}

const TABLE_SRC: &str = include_str!("i18n_ui.tsv");

// Spalten der Tabelle: Englisch, Latein, Deutsch mit Lehnwoertern.
const COLUMN_LOANWORDS: usize = 2;

struct Table {
    // Woertliche Texte -> [Englisch, Latein, Deutsch mit Lehnwoertern].
    exact: HashMap<&'static str, [&'static str; 3]>,
    // Eintraege mit "{}": Stuecke zwischen den Platzhaltern und die Zielmuster.
    templates: Vec<(Vec<&'static str>, [&'static str; 3])>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut exact = HashMap::default();
        let mut templates = Vec::new();
        for (de, en, la) in entries() {
            // Einmal beim Aufbau, fuer die Laufzeit des Programms -- die Tabelle lebt ohnehin so
            // lange.
            let lehn = with_loanwords(de);
            let lehn: &'static str = if lehn == de { de } else { Box::leak(lehn.into_boxed_str()) };
            if de.contains("{}") {
                templates.push((de.split("{}").collect(), [en, la, lehn]));
            } else {
                exact.entry(de).or_insert([en, la, lehn]);
            }
        }
        // Laengere Vorlagen zuerst: "Skin konnte nicht geladen werden: {}" vor "{} ...".
        templates.sort_by_key(|(stuecke, _): &(Vec<&str>, _)| std::cmp::Reverse(stuecke.iter().map(|s| s.len()).sum::<usize>()));
        Table { exact, templates }
    })
}

fn entries() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    TABLE_SRC.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).filter_map(|l| {
        let mut teile = l.split('\t');
        Some((teile.next()?, teile.next()?, teile.next()?))
    })
}

// Ergebnisse je Spalte und Text, auch die Fehlschlaege -- shape_text fragt jeden Frame nach
// denselben paar hundert Texten, und die Vorlagen durchzuprobieren kostet.
static CACHE: Mutex<Option<HashMap<(usize, String), Option<String>>>> = Mutex::new(None);
const CACHE_MAX: usize = 8192;

// Text der Oberflaeche in der eingestellten Sprache. Unbekanntes bleibt, wie es ist.
pub fn tr(text: &str) -> Cow<'_, str> {
    let lang = ui_language();
    if lang == Language::German && GERMAN_LOANWORDS.load(Ordering::Relaxed) {
        return tr_column(COLUMN_LOANWORDS, text);
    }
    let t = tr_in(lang, text);
    if lang == Language::Latin && !LATIN_MACRONS.load(Ordering::Relaxed) && t.chars().any(|c| strip_macrons(&c.to_string()) != c.to_string()) {
        return Cow::Owned(strip_macrons(&t));
    }
    t
}

pub fn tr_in(lang: Language, text: &str) -> Cow<'_, str> {
    let spalte = match lang {
        Language::German => return Cow::Borrowed(text),
        Language::English => 0,
        Language::Latin => 1,
    };
    tr_column(spalte, text)
}

fn tr_column(spalte: usize, text: &str) -> Cow<'_, str> {
    if text.is_empty() {
        return Cow::Borrowed(text);
    }
    let tabelle = table();
    if let Some(z) = tabelle.exact.get(text) {
        return Cow::Borrowed(z[spalte]);
    }
    let schluessel = (spalte, text.to_string());
    if let Ok(mut guard) = CACHE.lock() {
        let cache = guard.get_or_insert_with(HashMap::default);
        if let Some(treffer) = cache.get(&schluessel) {
            return treffer.clone().map_or(Cow::Borrowed(text), Cow::Owned);
        }
        let ergebnis = compose(tabelle, spalte, text);
        if cache.len() >= CACHE_MAX {
            cache.clear();
        }
        cache.insert(schluessel, ergebnis.clone());
        return ergebnis.map_or(Cow::Borrowed(text), Cow::Owned);
    }
    compose(tabelle, spalte, text).map_or(Cow::Borrowed(text), Cow::Owned)
}

fn exact(tabelle: &Table, spalte: usize, text: &str) -> Option<&'static str> {
    tabelle.exact.get(text).map(|z| z[spalte])
}

// Zusammengesetzte Texte: Einrueckung, Vorlagen mit Werten, "Beschriftung: Wert".
fn compose(tabelle: &Table, spalte: usize, text: &str) -> Option<String> {
    let kern = text.trim_start();
    if kern.len() != text.len() {
        let einzug = &text[..text.len() - kern.len()];
        return compose_core(tabelle, spalte, kern).map(|t| format!("{einzug}{t}"));
    }
    compose_core(tabelle, spalte, text)
}

fn compose_core(tabelle: &Table, spalte: usize, text: &str) -> Option<String> {
    if let Some(t) = exact(tabelle, spalte, text) {
        return Some(t.to_string());
    }
    for (stuecke, ziel) in &tabelle.templates {
        if let Some(werte) = match_template(stuecke, text) {
            // Eingesetzte Werte, die selbst Beschriftungen sind ("Status: Verbunden"), mit.
            let werte: Vec<&str> = werte.iter().map(|w| exact(tabelle, spalte, w).unwrap_or(w)).collect();
            return Some(fill_template(ziel[spalte], &werte));
        }
    }
    if let Some((vorne, hinten)) = text.split_once(": ") {
        let v = exact(tabelle, spalte, vorne);
        let h = exact(tabelle, spalte, hinten);
        if v.is_some() || h.is_some() {
            return Some(format!("{}: {}", v.unwrap_or(vorne), h.unwrap_or(hinten)));
        }
    }
    None
}

// Passt `text` auf die Stuecke einer Vorlage? Dann die eingesetzten Werte, der Reihe nach.
fn match_template<'a>(stuecke: &[&str], text: &'a str) -> Option<Vec<&'a str>> {
    let (erstes, rest) = stuecke.split_first()?;
    let (letztes, mitte) = rest.split_last()?;
    let mut rest = text.strip_prefix(erstes)?;
    rest = rest.strip_suffix(letztes)?;
    let mut werte = Vec::with_capacity(stuecke.len() - 1);
    for stueck in mitte {
        if stueck.is_empty() {
            return None;
        }
        let pos = rest.find(stueck)?;
        werte.push(&rest[..pos]);
        rest = &rest[pos + stueck.len()..];
    }
    werte.push(rest);
    // Ein leerer Wert ist fast immer ein Zufallstreffer ("{} ms" auf " ms").
    if werte.iter().any(|w| w.is_empty()) {
        return None;
    }
    Some(werte)
}

fn fill_template(ziel: &str, werte: &[&str]) -> String {
    let mut out = String::with_capacity(ziel.len() + 16);
    let mut werte = werte.iter();
    let mut teile = ziel.split("{}");
    if let Some(erstes) = teile.next() {
        out.push_str(erstes);
    }
    for teil in teile {
        out.push_str(werte.next().copied().unwrap_or(""));
        out.push_str(teil);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jede_zeile_hat_drei_spalten_und_gleich_viele_platzhalter() {
        for (nr, zeile) in TABLE_SRC.lines().enumerate() {
            if zeile.starts_with('#') || zeile.is_empty() {
                continue;
            }
            let teile: Vec<&str> = zeile.split('\t').collect();
            assert_eq!(teile.len(), 3, "Zeile {}: {zeile:?}", nr + 1);
            let n = teile[0].matches("{}").count();
            assert_eq!(teile[1].matches("{}").count(), n, "Englisch, Zeile {}", nr + 1);
            assert_eq!(teile[2].matches("{}").count(), n, "Latein, Zeile {}", nr + 1);
            assert!(teile.iter().all(|t| !t.trim().is_empty()), "leere Spalte in Zeile {}", nr + 1);
        }
    }

    #[test]
    fn doppelte_eintraege_widersprechen_sich_nicht() {
        let mut gesehen: HashMap<&str, (&str, &str)> = HashMap::default();
        for (de, en, la) in entries() {
            if let Some(&(e, l)) = gesehen.get(de) {
                assert_eq!((e, l), (en, la), "{de:?} doppelt mit anderer Übersetzung");
            }
            gesehen.insert(de, (en, la));
        }
    }

    // Eine Uebersetzung darf nicht selbst wieder ein deutscher Schluessel mit ANDERER
    // Bedeutung sein -- fit_text uebersetzt vorab, shape_text danach noch einmal.
    #[test]
    fn uebersetzung_wird_nicht_doppelt_uebersetzt() {
        let t = table();
        for (de, en, la) in entries() {
            let lehn = with_loanwords(de);
            for (spalte, ziel) in [(0, en), (1, la), (COLUMN_LOANWORDS, lehn.as_str())] {
                if let Some(z) = t.exact.get(ziel) {
                    assert_eq!(z[spalte], ziel, "{ziel:?} würde ein zweites Mal übersetzt");
                }
            }
        }
    }

    #[test]
    fn woertlich_vorlage_beschriftung_und_einzug() {
        assert_eq!(tr_in(Language::English, "Aussehen"), "Appearance");
        assert_eq!(tr_in(Language::German, "Aussehen"), "Aussehen");
        assert_eq!(tr_in(Language::Latin, "← Zurück"), "← Redīre");
        assert_eq!(tr_in(Language::English, "Verbinde mit play.example.net ..."), "Connecting to play.example.net ...");
        assert_eq!(tr_in(Language::English, "Status: Partikel"), "Status: Particles");
        assert_eq!(tr_in(Language::English, "  Deckkraft"), "  Opacity");
        assert_eq!(tr_in(Language::English, "irgendwas Unbekanntes"), "irgendwas Unbekanntes");
        assert_eq!(tr_in(Language::English, "Skin geladen: CPVP + Anderes"), "Skin loaded: CPVP + Anderes");
    }

    #[test]
    fn lehnwoerter_nur_in_texten_der_oberflaeche() {
        assert_eq!(tr_column(COLUMN_LOANWORDS, "Sichtfeld (Ego)"), "FOV (Ego)");
        assert_eq!(tr_column(COLUMN_LOANWORDS, "Mit Zweithand tauschen"), "Mit Offhand tauschen");
        assert_eq!(tr_column(COLUMN_LOANWORDS, "Schild-Cooldown"), "Shield-Cooldown");
        // Das Schild zum Beschriften bleibt.
        assert_eq!(tr_column(COLUMN_LOANWORDS, "Schild bearbeiten"), "Schild bearbeiten");
        // Fremder Text (Chat, Itemnamen) bleibt unangetastet.
        assert_eq!(tr_column(COLUMN_LOANWORDS, "mein Fadenkreuz ist kaputt"), "mein Fadenkreuz ist kaputt");
        assert_eq!(tr_column(COLUMN_LOANWORDS, LOANWORDS_TOOLTIP), LOANWORDS_TOOLTIP);
    }
}
