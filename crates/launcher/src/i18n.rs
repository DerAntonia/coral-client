// Sprache des Launchers (Seite Launcher -> Sprache), unabhaengig vom Client. Wie dort ist die
// Oberflaeche auf Deutsch geschrieben; tr() uebersetzt einen fertigen Text, der woertlich in
// i18n.tsv steht. Eintraege mit "{}" sind Vorlagen: die eingesetzten Werte bleiben, sind sie
// selbst Beschriftungen, werden sie mit uebersetzt ("Neue Version: {}").

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    German,
    English,
    Latin,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::German, Language::English, Language::Latin];

    // In der Sprache selbst -- wer die Oberflaeche nicht lesen kann, findet so zurueck.
    pub fn label(self) -> &'static str {
        match self {
            Language::German => "Deutsch",
            Language::English => "English",
            Language::Latin => "Latīna",
        }
    }

    // Wert in launcher.conf.
    pub fn code(self) -> &'static str {
        match self {
            Language::German => "de",
            Language::English => "en",
            Language::Latin => "la",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.code() == code)
    }
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(lang: Language) {
    LANGUAGE.store(lang as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    Language::ALL[LANGUAGE.load(Ordering::Relaxed) as usize]
}

const TABLE_SRC: &str = include_str!("i18n.tsv");

struct Table {
    // Woertliche Texte -> [Englisch, Latein].
    exact: HashMap<&'static str, [&'static str; 2]>,
    // Vorlagen: Stuecke zwischen den Platzhaltern und die Zielmuster, laengere zuerst.
    templates: Vec<(Vec<&'static str>, [&'static str; 2])>,
}

fn entries() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    TABLE_SRC.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).filter_map(|l| {
        let mut teile = l.split('\t');
        Some((teile.next()?, teile.next()?, teile.next()?))
    })
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut exact = HashMap::new();
        let mut templates = Vec::new();
        for (de, en, la) in entries() {
            if de.contains("{}") {
                templates.push((de.split("{}").collect::<Vec<_>>(), [en, la]));
            } else {
                exact.entry(de).or_insert([en, la]);
            }
        }
        templates.sort_by_key(|(stuecke, _)| std::cmp::Reverse(stuecke.iter().map(|s| s.len()).sum::<usize>()));
        Table { exact, templates }
    })
}

/// Text in der eingestellten Sprache. Unbekanntes bleibt, wie es ist.
pub fn tr(text: &str) -> Cow<'_, str> {
    let spalte = match language() {
        Language::German => return Cow::Borrowed(text),
        Language::English => 0,
        Language::Latin => 1,
    };
    let tabelle = table();
    if let Some(z) = tabelle.exact.get(text) {
        return Cow::Borrowed(z[spalte]);
    }
    for (stuecke, ziel) in &tabelle.templates {
        if let Some(werte) = match_template(stuecke, text) {
            let werte: Vec<&str> = werte.iter().map(|w| tabelle.exact.get(w).map_or(*w, |z| z[spalte])).collect();
            return Cow::Owned(fill_template(ziel[spalte], &werte));
        }
    }
    Cow::Borrowed(text)
}

// Passt `text` auf die Stuecke einer Vorlage? Dann die eingesetzten Werte, der Reihe nach.
fn match_template<'a>(stuecke: &[&str], text: &'a str) -> Option<Vec<&'a str>> {
    let (erstes, rest) = stuecke.split_first()?;
    let (letztes, mitte) = rest.split_last()?;
    let mut rest = text.strip_prefix(erstes)?.strip_suffix(letztes)?;
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
    // Ein leerer Wert ist fast immer ein Zufallstreffer.
    if werte.iter().any(|w| w.is_empty()) {
        return None;
    }
    Some(werte)
}

fn fill_template(ziel: &str, werte: &[&str]) -> String {
    let mut teile = ziel.split("{}");
    let mut out = teile.next().unwrap_or("").to_string();
    for (teil, wert) in teile.zip(werte.iter().copied().chain(std::iter::repeat(""))) {
        out.push_str(wert);
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
            assert!(teile[1..].iter().all(|t| t.matches("{}").count() == n), "Platzhalter, Zeile {}", nr + 1);
            assert!(teile.iter().all(|t| !t.trim().is_empty()), "leere Spalte in Zeile {}", nr + 1);
        }
    }

    #[test]
    fn woertlich_und_vorlage() {
        set_language(Language::English);
        assert_eq!(tr("Abbrechen"), "Cancel");
        assert_eq!(tr("Neue Version: v1.2"), "New version: v1.2");
        assert_eq!(tr("Unbekannt"), "Unbekannt");
        set_language(Language::Latin);
        assert_eq!(tr("Abbrechen"), "Abrogāre");
        set_language(Language::German);
        assert_eq!(tr("Abbrechen"), "Abbrechen");
    }
}
