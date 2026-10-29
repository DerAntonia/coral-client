// Die Klänge der Oberfläche.
//
// Gespielt werden AUFNAHMEN, zugeordnet in `sample()`: je Platz eine Datei, die der Nutzer im
// Sound-Baukasten aus Kenney-Aufnahmen (CC0) und eigener Synthese gebaut hat (s.
// assets/sounds/custom/README.md). Nicht aus Vanilla entliehen: Coral Client soll klanglich eine eigene
// Identität haben, und daran ändern fremde UI-Klänge nichts -- sie klingen nach niemandem.
//
// Der synthetische Weg (`cue()`) bleibt vollständig bestehen und ist der Rückfall: er greift,
// wenn eine Aufnahme fehlt, und `sample()` auf None zu setzen tauscht einen einzelnen Klang
// zurück. Er stand hier zuerst allein -- bis sich zeigte, dass sich aus Dauer, Hüllkurve und
// Helligkeit zwar ein KORREKTER Klang bauen lässt, aber kein guter: das entscheidet das Ohr,
// und ich habe keins. Deshalb jetzt Aufnahmen, bei denen jemand mit Ohren die Arbeit gemacht
// hat.
//
// Acht Typen für achtzig Aktionen. Die Zuordnung ist absichtlich grob: eine Oberfläche wird
// nicht dadurch verständlich, dass jeder Knopf anders klingt, sondern dadurch, dass gleiche
// ARTEN von Ereignissen gleich klingen. Man lernt vier oder fünf Klänge, nicht achtzig.

use audio::Cue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiSound {
    // Ein klickbares Element wurde BETRETEN (nicht: die Maus bewegt sich darüber). Der
    // häufigste Klang von allen -- entsprechend kurz und leise.
    Hover,
    // Eine Aktion wurde ausgeführt.
    Click,
    // Ein Schalter ging an bzw. aus.
    ToggleOn,
    ToggleOff,
    // Eine Fläche wurde geöffnet bzw. geschlossen.
    Open,
    Close,
    // Ein Schritt in einer Reihe: Stepper, Hotbar-Platz, Vorschlagsliste, Chat-Verlauf.
    Step,
    // Die Aktion war nicht möglich (abgeblendeter Knopf).
    Denied,
    // Ein Zeichen wurde getippt. Noch häufiger als Hover und entsprechend noch
    // zurückhaltender -- aber NICHT stumm: Tippen ist eine bewusste Handlung, und Rückmeldung
    // darauf ist erwünscht (osu!lazer macht es genauso). Was es erträglich macht, ist
    // dasselbe wie beim Hover: sehr leise, sehr kurz, deutlich gestreut.
    Type,
    // Zeichen gelöscht -- dumpfer als das Tippen, damit sich Löschen vom Schreiben
    // unterscheidet, ohne eine eigene Aufmerksamkeit zu verlangen.
    Erase,
    // Ein Schieberegler hat seinen Wert geändert. Die Tonhöhe steigt mit dem Wert (s.
    // main.rs::SLIDER_TICK_RISE_SEMITONES), so hört man, wo auf der Bahn man ist.
    Slider,
    // Ein Eintrag erscheint, wie die Zeilen der Rangliste in osu: kommt in schnellen Folgen
    // (je Zeile einer), deshalb kurz und leise.
    Popup,
    // Eine Chat-Nachricht ging raus: normaler Chat oder ein Nachrichten-Befehl (/msg, /tc, ...,
    // s. chat_input::sends_message) -- nicht jeder andere Befehl.
    Send,
}

impl UiSound {
    /// Alle Klänge, in der Reihenfolge der Lautstärke-Regler im Audio-Tab (s.
    /// main.rs::Settings::ui_sound_volume).
    pub const ALL: [UiSound; 13] = [
        UiSound::Hover,
        UiSound::Click,
        UiSound::ToggleOn,
        UiSound::ToggleOff,
        UiSound::Open,
        UiSound::Close,
        UiSound::Step,
        UiSound::Denied,
        UiSound::Type,
        UiSound::Erase,
        UiSound::Slider,
        UiSound::Popup,
        UiSound::Send,
    ];

    /// Stelle in `ALL`.
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }

    /// Schlüssel in der Config (`ui_volume_<key>`).
    pub fn key(self) -> &'static str {
        match self {
            UiSound::Hover => "hover",
            UiSound::Click => "click",
            UiSound::ToggleOn => "toggle_on",
            UiSound::ToggleOff => "toggle_off",
            UiSound::Open => "open",
            UiSound::Close => "close",
            UiSound::Step => "step",
            UiSound::Denied => "denied",
            UiSound::Type => "type",
            UiSound::Erase => "erase",
            UiSound::Slider => "slider",
            UiSound::Popup => "popup",
            UiSound::Send => "send",
        }
    }

    /// Name im Audio-Tab, wie im Sound-Baukasten.
    pub fn label(self) -> &'static str {
        match self {
            UiSound::Hover => "Hover",
            UiSound::Click => "Klick",
            UiSound::ToggleOn => "Schalter an",
            UiSound::ToggleOff => "Schalter aus",
            UiSound::Open => "Öffnen",
            UiSound::Close => "Schließen",
            UiSound::Step => "Schritt",
            UiSound::Denied => "Gesperrt",
            UiSound::Type => "Tippen",
            UiSound::Erase => "Löschen",
            UiSound::Slider => "Slider-Tick",
            UiSound::Popup => "Aufploppen",
            UiSound::Send => "Absenden",
        }
    }

    /// Wann er spielt -- als Erklärung beim Draufzeigen im Audio-Tab.
    pub fn tooltip(self) -> &'static str {
        match self {
            UiSound::Hover => "Lautstärke des Hover-Klangs: die Maus betritt ein klickbares Element. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Click => "Lautstärke des Klicks: eine Aktion wird ausgeführt. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::ToggleOn => "Lautstärke beim Einschalten eines Schalters. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::ToggleOff => "Lautstärke beim Ausschalten eines Schalters. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Open => "Lautstärke, wenn eine Fläche oder ein Fenster aufgeht. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Close => "Lautstärke, wenn eine Fläche oder ein Fenster zugeht. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Step => "Lautstärke der Schritte: Stepper, Hotbar-Platz, Vorschlagsliste, Chat-Verlauf. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Denied => "Lautstärke, wenn eine Aktion nicht möglich ist (abgeblendeter Knopf). Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Type => "Lautstärke des Tippens: ein Zeichen wurde getippt. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Erase => "Lautstärke des Löschens: ein Zeichen wurde gelöscht. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Slider => "Lautstärke des Slider-Ticks: ein Schieberegler ändert seinen Wert, die Tonhöhe steigt mit dem Wert. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Popup => "Lautstärke des Aufploppens: ein Eintrag erscheint, etwa eine Zeile einer Rangliste. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
            UiSound::Send => "Lautstärke beim Absenden einer Chat-Nachricht, auch per /msg, /w, /tc und Ähnlichem. Relativ zu UI-Sounds oben. Ändern spielt ihn zur Probe.",
        }
    }

    /// Die Aufnahme, die für diesen Klang gespielt wird -- Name wie in `audio::bank`.
    ///
    /// Alle stammen aus dem Sound-Baukasten: dort hat der Nutzer jeden Platz nach Gehör gebaut
    /// (Ebenen, Tonhöhe, Klangformung, Limiter), exportiert hat ihn die Seite genau so, wie er
    /// dort klingt (s. assets/sounds/custom/README.md). Gesperrt ist dort leer und bleibt
    /// stumm. `None` = stumm, nicht synthetisieren: die Synthese (`cue()`) war schon einmal
    /// verworfen worden.
    pub fn sample(self) -> Option<&'static str> {
        Some(match self {
            UiSound::Hover => "custom/ui/ui_hover",
            UiSound::Click => "custom/ui/ui_click",
            UiSound::ToggleOn => "custom/ui/ui_toggle_on",
            UiSound::ToggleOff => "custom/ui/ui_toggle_off",
            UiSound::Open => "custom/ui/ui_open",
            UiSound::Close => "custom/ui/ui_close",
            UiSound::Step => "custom/ui/ui_step",
            UiSound::Denied => return None,
            UiSound::Type => "custom/ui/ui_type",
            UiSound::Erase => "custom/ui/ui_erase",
            UiSound::Slider => "custom/ui/ui_slider",
            UiSound::Popup => "custom/ui/ui_popup",
            UiSound::Send => "custom/ui/ui_send",
        })
    }

    /// Streuung der Tonhöhe in Halbtönen: bei jedem Abspielen gleichverteilt in ±diesem Wert,
    /// wie im Baukasten. Dort eingestellt, nur bei den Klängen, die in schnellen Folgen kommen.
    pub fn spread_semitones(self) -> f32 {
        match self {
            UiSound::Hover | UiSound::Step => 0.75,
            UiSound::Type => 1.5,
            UiSound::Send => 0.5,
            _ => 0.0,
        }
    }

    /// Mindestabstand zwischen zwei Mal demselben Klang -- schneller Folgendes wird geschluckt,
    /// sonst stapeln sich beim Überstreichen einer Liste oder beim Ziehen eines Reglers die
    /// Stimmen. Werte wie im Baukasten.
    pub fn min_gap(self) -> std::time::Duration {
        std::time::Duration::from_millis(match self {
            UiSound::Hover => 30,
            UiSound::Slider => 35,
            UiSound::Step => 25,
            UiSound::Type | UiSound::Erase => 15,
            _ => 0,
        })
    }

    /// Klänge, die beim Halten einer Taste mit der Tastenwiederholung kommen. Dort stapelten
    /// sich die Stimmen (Löschen klingt bis zu 200 ms nach, die Wiederholung kommt alle 30 ms)
    /// und es wurde sehr laut. Deshalb drückt jeder Anschlag den nächsten leiser, mit der Zeit
    /// kommt die Lautstärke zurück (s. `repeat_level`).
    pub fn ducks(self) -> bool {
        matches!(self, UiSound::Type | UiSound::Erase)
    }

    /// Obergrenze für die Länge der Aufnahme. Dieselbe Regel wie bei der Synthese: was oft
    /// kommt, sollte kurz sein, sonst stapeln sich die Klänge übereinander. 70 ms für die
    /// Klänge in schnellen Folgen; Löschen kommt seltener am Stück, die übrigen einzeln.
    #[cfg(test)]
    pub fn max_sample_ms(self) -> f32 {
        match self {
            UiSound::Hover | UiSound::Step | UiSound::Type | UiSound::Slider | UiSound::Popup => 70.0,
            UiSound::Erase => 200.0,
            _ => 400.0,
        }
    }

    /// Untergrenze für die Länge der Aufnahme: darunter ist ein Klang praktisch unhörbar
    /// (mehrere Rohdateien im Kenney-Satz sind 7 bis 12 ms lang). Der kürzeste gehörte und
    /// bestätigte Klang ist der Slider-Tick mit 17 ms.
    #[cfg(test)]
    pub fn min_sample_ms(self) -> f32 {
        10.0
    }

    /// Lautstärke der Aufnahme, 1.0 = unverändert.
    ///
    /// War hier einmal 0.22 -- eine Zahl, die sich niemand angehört hat. Gewollt ist
    /// exakt das, was `scripts/klaenge-anhoeren.sh` über paplay hören lässt, und das spielt
    /// die Datei OHNE Abschwächung; 1.0 lässt zusätzlich Kopfraum bis zur Aussteuerungsgrenze
    /// (`fill_buffer` klemmt ohnehin auf -1..1, falls mehrere Stimmen sich addieren).
    ///
    /// Der Pegel steckt schon in der Datei: der Baukasten exportiert jeden Platz so laut, wie
    /// er dort klingt.
    ///
    /// Das Aufploppen ist lauter gewuenscht (2026-10-15: 250 %), seit es auch beim Markieren
    /// jedes Zeichens kommt; die Datei hat Spitzen um 0,03, das bleibt weit unter der Grenze.
    pub fn sample_gain(self) -> f32 {
        match self {
            UiSound::Popup => 2.5,
            _ => 1.0,
        }
    }

    // Wie stark Tonhöhe und Lautstärke gestreut werden. Nur beim Hover nennenswert: er kommt
    // in schnellen Folgen (Überstreichen einer Liste), und ohne Streuung hört man dann die
    // Wiederholung statt einer Textur. Die übrigen Klänge kommen einzeln -- dort wäre
    // Streuung nur Unruhe.
    //
    // Aktuell nur von Tests aufgerufen: `play_ui_sound` spielt keine synthetischen Klänge mehr
    // (s. dortiger Kommentar). Bewusst nicht gelöscht -- die Tests dokumentieren ein
    // durchdachtes Klangkonzept, das zurückkommt, sobald für einen weiteren Klang eine
    // Aufnahme feststeht und `cue()` wieder als Rückfall gebraucht wird.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn variation(self) -> f32 {
        match self {
            UiSound::Hover => 0.10,
            // Beim Tippen die stärkste Streuung von allen: eine Folge gleicher Anschläge in
            // Schreibgeschwindigkeit wäre das Maschinengewehr, vor dem ich beim Hover
            // fälschlich gewarnt habe. Mit Streuung wird daraus ein Tastaturgeräusch.
            UiSound::Type => 0.14,
            UiSound::Erase => 0.10,
            UiSound::Step | UiSound::Slider => 0.04,
            _ => 0.0,
        }
    }

    // Die Klangbeschreibung. Die Zahlen sind der Teil, den man mit den Ohren nachjustiert --
    // die Struktur dahinter nicht.
    //
    // Aktuell nur von Tests aufgerufen, s. Kommentar an `variation`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn cue(self) -> Cue {
        match self {
            // Sehr kurz, sehr leise, hoch. Muss beim Überstreichen einer Liste zu einem
            // Rascheln verschmelzen, nicht zu einer Folge erkennbarer Töne.
            UiSound::Hover => Cue { hz_start: 1512.0, hz_end: 1512.0, noise: 0.25, attack: 0.001, decay: 0.024, gain: 0.055, pan: 0.0 },
            // Trocken und breitbandig. Der hohe Rauschanteil ist der Grund, warum es nach
            // Klick klingt und nicht nach Piep -- ein Klick ist ein Geräusch, kein Ton.
            UiSound::Click | UiSound::Send => Cue { hz_start: 1260.0, hz_end: 1080.0, noise: 0.72, attack: 0.0015, decay: 0.032, gain: 0.20, pan: 0.0 },
            // Schalter: derselbe trockene Anschlag, aber mit deutlicher Richtung. Steigend =
            // an, fallend = aus. Die Richtung trägt die Information, nicht die Tonhöhe --
            // deshalb reicht dasselbe Intervall in beide Richtungen.
            UiSound::ToggleOn => Cue { hz_start: 648.0, hz_end: 1080.0, noise: 0.30, attack: 0.0015, decay: 0.055, gain: 0.18, pan: 0.0 },
            UiSound::ToggleOff => Cue { hz_start: 1080.0, hz_end: 648.0, noise: 0.30, attack: 0.0015, decay: 0.055, gain: 0.18, pan: 0.0 },
            // Flächen: dieselbe Idee eine Oktave tiefer und deutlich länger -- das
            // "Aufploppen". Öffnen und Schließen sind exakt gespiegelt.
            UiSound::Open => Cue { hz_start: 302.0, hz_end: 605.0, noise: 0.10, attack: 0.003, decay: 0.085, gain: 0.22, pan: 0.0 },
            UiSound::Close => Cue { hz_start: 605.0, hz_end: 302.0, noise: 0.10, attack: 0.003, decay: 0.085, gain: 0.22, pan: 0.0 },
            // Neutraler Tick ohne Richtung: er sagt "eins weiter", nicht "an" oder "aus".
            UiSound::Step | UiSound::Slider | UiSound::Popup => Cue { hz_start: 1008.0, hz_end: 1008.0, noise: 0.45, attack: 0.001, decay: 0.020, gain: 0.10, pan: 0.0 },
            // Tief und stumpf. Muss sich klar vom Klick unterscheiden, ohne unangenehm zu
            // sein -- man löst ihn versehentlich aus, nicht absichtlich.
            UiSound::Denied => Cue { hz_start: 230.0, hz_end: 187.0, noise: 0.20, attack: 0.002, decay: 0.090, gain: 0.16, pan: 0.0 },
            // Trocken und sehr kurz, fast reines Rauschen -- der Anschlag einer Taste ist ein
            // Klacken, kein Ton. Leiser als der Hover, weil in Schreibgeschwindigkeit noch
            // mehr davon kommen.
            UiSound::Type => Cue { hz_start: 1728.0, hz_end: 1368.0, noise: 0.85, attack: 0.0008, decay: 0.016, gain: 0.045, pan: 0.0 },
            UiSound::Erase => Cue { hz_start: 864.0, hz_end: 648.0, noise: 0.75, attack: 0.0008, decay: 0.022, gain: 0.055, pan: 0.0 },
        }
    }
}

// Misst, was sich an einem Klang OBJEKTIV bestimmen laesst -- Dauer, Lautheit, Helligkeit,
// Schaerfe. #[ignore], weil es ein Werkzeug zum Abstimmen ist und keine Zusicherung:
//   cargo test -p app ui_sound::mess -- --ignored --nocapture
//
// Der Anlass: ob ein Klang "angenehm" ist, entscheidet das Ohr und nicht der Code. Diese vier
// Zahlen decken aber genau die Fragen ab, um die es beim Nachjustieren geht ("zu hell", "zu
// laut", "zu lang", "zu scharf") und machen aus einem Bauchgefuehl einen vergleichbaren Wert.
//
// Helligkeit ist der SPEKTRALE SCHWERPUNKT: der mit der Energie gewichtete Mittelwert der
// Frequenzen. Hoch = hell/zischend, niedrig = dumpf. Schaerfe ist der Scheitelfaktor (Spitze
// zu Effektivwert): hoch = knackig/impulsiv, niedrig = gleichmaessig/toenend.
//
// Frueher lag das hier als eigener Integrationstest im audio-Crate und hatte die Klangtabelle
// samt Mixer-Formel als KOPIE. Beides ist inzwischen die echte Quelle: die Klaenge kommen aus
// cue() gleich nebenan, das Ausrechnen aus audio::render_cue. Eine Kopie, die still veraltet,
// misst irgendwann etwas, das niemand hoert.
/// Der nächste Anschlag ist so viel leiser als der letzte (Faktor auf dessen Lautstärke) ...
const DUCK_STEP: f32 = 0.5;
/// ... und kehrt mit dieser Zeitkonstante (Sekunden) zum Standard zurück.
const DUCK_RECOVER_S: f32 = 0.15;

/// Lautstärke-Faktor eines Anschlags `seit` Sekunden nach dem letzten, der mit `vorher` gespielt
/// wurde. Ergibt bei gehaltener Taste (30 je Sekunde) gut ein Drittel, beim Schreiben (zehn je
/// Sekunde) rund zwei Drittel, nach einer Pause wieder den vollen Pegel.
pub fn repeat_level(vorher: f32, seit: f32) -> f32 {
    let danach = vorher * DUCK_STEP;
    1.0 - (1.0 - danach) * (-seit.max(0.0) / DUCK_RECOVER_S).exp()
}

#[cfg(test)]
mod mess {
    use super::tests::ALLE;

    // Grobe DFT -- fuer den Trend ("heller als der andere") reicht das voellig.
    fn helligkeit(samples: &[f32], rate: f32) -> f32 {
        let (mut summe, mut gewicht) = (0.0f64, 0.0f64);
        let mut hz = 120.0f32;
        while hz < rate * 0.45 {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, s) in samples.iter().enumerate() {
                let w = std::f64::consts::TAU * hz as f64 * i as f64 / rate as f64;
                re += *s as f64 * w.cos();
                im += *s as f64 * w.sin();
            }
            let energie = (re * re + im * im).sqrt();
            summe += energie * hz as f64;
            gewicht += energie;
            hz *= 1.12;
        }
        if gewicht > 0.0 {
            (summe / gewicht) as f32
        } else {
            0.0
        }
    }

    #[test]
    #[ignore]
    fn berichtet_die_klangeigenschaften() {
        let rate = 48_000.0;
        println!("{:<10} {:>7} {:>8} {:>10} {:>8}", "Klang", "ms", "Lautheit", "Helligkeit", "Schärfe");
        println!("{}", "-".repeat(48));
        let mut sortiert: Vec<_> = ALLE.iter().map(|s| (*s, s.cue())).collect();
        sortiert.sort_by(|a, b| a.1.gain.total_cmp(&b.1.gain));
        for (sound, cue) in sortiert {
            let s = audio::render_cue(cue, rate);
            let rms = (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt();
            let spitze = s.iter().fold(0.0f32, |a, v| a.max(v.abs()));
            let scheitel = if rms > 0.0 { spitze / rms } else { 0.0 };
            println!(
                "{:<10} {:>7.1} {:>8.4} {:>9.0}Hz {:>8.2}",
                format!("{sound:?}"),
                cue.duration() * 1000.0,
                rms,
                helligkeit(&s, rate),
                scheitel
            );
        }
        println!("\nHelligkeit: höher = heller/zischender. Schärfe: höher = knackiger.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) const ALLE: [UiSound; UiSound::ALL.len()] = UiSound::ALL;

    #[test]
    // Jeder Name in der Zuordnungstabelle muss auch wirklich im Bestand liegen. Ohne diesen
    // Test faellt ein Tippfehler NICHT auf: play_ui_sound weicht bei einem unbekannten Namen
    // stillschweigend auf den synthetischen Klang aus -- man hoerte etwas, nur eben das
    // Falsche, und wuerde die Aufnahme fuer schlecht halten statt fuer nicht geladen.
    fn jede_zuordnung_zeigt_auf_eine_vorhandene_aufnahme() {
        let bank = audio::bank::SoundBank::new();
        for sound in ALLE {
            let Some(name) = sound.sample() else { continue };
            assert!(bank.get(name).is_some(), "{sound:?} zeigt auf \"{name}\" -- das gibt es nicht");
        }
    }

    #[test]
    // Die Regel "je häufiger, desto kürzer" gilt für Aufnahmen genauso wie für die Synthese --
    // und wurde bei der ersten Zuordnung komplett verfehlt: Hover lag bei 224 ms, die Schalter
    // bei über 600 ms. Beim Überstreichen einer Liste stapeln sich davon bis zu acht Stimmen
    // (audio::MAX_VOICES) übereinander.
    //
    // Der Test misst die ECHTEN Dateien, nicht eine Angabe daneben -- eine Angabe hätte
    // denselben Fehler noch einmal machen können.
    fn aufnahmen_sind_kurz_genug_fuer_ihre_haeufigkeit() {
        let bank = audio::bank::SoundBank::new();
        for sound in ALLE {
            let Some(name) = sound.sample() else { continue };
            let w = bank.get(name).unwrap_or_else(|| panic!("{name} fehlt"));
            let ms = w.duration() * 1000.0;
            assert!(ms <= sound.max_sample_ms(), "{sound:?} spielt \"{name}\" mit {ms:.0} ms, erlaubt sind {:.0}", sound.max_sample_ms());
            assert!(
                ms >= sound.min_sample_ms(),
                "{sound:?} spielt \"{name}\" mit nur {ms:.0} ms, erwartet mind. {:.0}",
                sound.min_sample_ms()
            );
        }
    }

    #[test]
    // Jeder Klang ausser Gesperrt hat eine Aufnahme aus dem Baukasten -- faellt eine Zuordnung
    // weg, ist der Klang still, und das merkt sonst niemand.
    fn alle_klaenge_ausser_gesperrt_haben_eine_aufnahme() {
        for sound in ALLE {
            assert_eq!(sound.sample().is_none(), sound == UiSound::Denied, "{sound:?}");
        }
    }

    #[test]
    // Die Reihenfolge in ALL ist die der Lautstaerke-Regler und der Config-Schluessel.
    fn index_und_schluessel_passen_zu_all() {
        for (i, sound) in UiSound::ALL.into_iter().enumerate() {
            assert_eq!(sound.index(), i);
        }
        let mut keys: Vec<_> = UiSound::ALL.iter().map(|s| s.key()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), UiSound::ALL.len(), "Schluessel doppelt");
    }

    #[test]
    // Alle Klänge müssen kurz genug sein, um die Oberfläche nicht zu verschleppen. Ein
    // UI-Klang, der länger dauert als die Aktion, fühlt sich träge an.
    fn alle_klaenge_sind_kurz() {
        for sound in ALLE {
            let dauer = sound.cue().duration();
            assert!(dauer > 0.005, "{sound:?} ist unhörbar kurz: {dauer}");
            assert!(dauer <= 0.1, "{sound:?} dauert zu lange: {dauer}");
        }
    }

    #[test]
    // Die Grundregel des ganzen Sets: JE HÄUFIGER ein Klang vorkommt, desto leiser muss er
    // sein. Tippen (zehn pro Sekunde) unter Hover (zwanzig beim Überstreichen einer Liste)
    // unter allem, was einzeln ausgelöst wird.
    //
    // Zwei frühere Fassungen dieses Tests waren zu eng formuliert: erst "Hover ist der
    // kürzeste" (falsch, der Schritt-Tick ist kürzer), dann "Hover ist der leiseste" (galt nur,
    // solange es keine Tippgeräusche gab). Die Rangfolge nach Häufigkeit ist die Regel, die
    // dahintersteht -- sie überlebt auch den nächsten neuen Klang.
    fn haeufigere_klaenge_sind_leiser() {
        let selten: Vec<UiSound> = ALLE.iter().copied().filter(|s| !matches!(s, UiSound::Type | UiSound::Erase | UiSound::Hover)).collect();
        let tippen = UiSound::Type.cue().gain;
        let hover = UiSound::Hover.cue().gain;
        assert!(tippen < hover, "Tippen muss leiser sein als Hover: {tippen} / {hover}");
        for sound in selten {
            assert!(hover < sound.cue().gain, "Hover muss leiser sein als {sound:?}");
        }
        // Und kurz genug, dass schnelle Folgen verschmelzen statt als Einzeltöne zu stehen.
        assert!(UiSound::Hover.cue().duration() <= 0.030);
        assert!(UiSound::Type.cue().duration() <= 0.020);
    }

    #[test]
    // Tippen ist der häufigste Klang überhaupt -- in Schreibgeschwindigkeit zehn pro Sekunde.
    // Er muss leiser sein als der Hover und am stärksten gestreut.
    fn tippen_ist_noch_zurueckhaltender_als_hover() {
        let tippen = UiSound::Type.cue();
        assert!(tippen.gain < UiSound::Hover.cue().gain, "Tippen muss leiser sein als Hover");
        assert!(tippen.duration() < UiSound::Hover.cue().duration(), "und kürzer");
        assert!(UiSound::Type.variation() > UiSound::Hover.variation(), "und stärker gestreut");
        // Löschen muss sich hörbar unterscheiden, sonst merkt man den Unterschied nicht.
        assert!(UiSound::Erase.cue().hz_start < tippen.hz_start * 0.7, "Löschen muss deutlich dumpfer sein");
    }

    #[test]
    // Öffnen und Schließen sind gespiegelt, ebenso An und Aus. Die RICHTUNG trägt die
    // Information -- wären beide gleich, hörte man nicht, was passiert ist.
    fn gegensatzpaare_sind_gespiegelt() {
        for (auf, zu) in [(UiSound::Open, UiSound::Close), (UiSound::ToggleOn, UiSound::ToggleOff)] {
            let (a, b) = (auf.cue(), zu.cue());
            assert!(a.hz_end > a.hz_start, "{auf:?} muss steigen");
            assert!(b.hz_end < b.hz_start, "{zu:?} muss fallen");
            assert_eq!(a.hz_start, b.hz_end, "gespiegelt: Anfang des einen = Ende des anderen");
            assert_eq!(a.hz_end, b.hz_start);
            assert_eq!(a.duration(), b.duration(), "gleiche Länge, sonst wirkt eine Richtung schwerer");
        }
    }

    #[test]
    // Klick und Schritt leben vom Rauschanteil: ohne ihn klingen sie wie ein Piepen. Die
    // Flächen-Klänge dagegen sollen tonal sein.
    fn klick_ist_geraeusch_flaeche_ist_ton() {
        assert!(UiSound::Click.cue().noise > 0.5, "der Klick muss überwiegend Rauschen sein");
        assert!(UiSound::Step.cue().noise > 0.3);
        assert!(UiSound::Open.cue().noise < 0.2, "das Aufploppen soll tonal sein");
        assert!(UiSound::Type.cue().noise > 0.7, "der Tastenanschlag ist ein Klacken, kein Ton");
    }

    #[test]
    // Nur der Hover (und schwächer der Schritt) wird gestreut -- die Einzelklänge nicht.
    fn nur_haeufige_klaenge_werden_gestreut() {
        assert!(UiSound::Hover.variation() > 0.05);
        assert_eq!(UiSound::Click.variation(), 0.0);
        assert_eq!(UiSound::Open.variation(), 0.0);
    }

    #[test]
    // Kein Klang darf übersteuern -- acht gleichzeitige Stimmen sind möglich (s. audio).
    fn lautstaerken_bleiben_im_rahmen() {
        for sound in ALLE {
            let gain = sound.cue().gain;
            assert!(gain > 0.0 && gain <= 0.3, "{sound:?}: {gain}");
        }
    }

    #[test]
    // Gehaltene Taste: deutlich leiser als der erste Anschlag; nach einer Pause wieder voll.
    fn wiederholung_wird_leiser_und_erholt_sich() {
        let mut pegel = 1.0;
        for _ in 0..60 {
            pegel = super::repeat_level(pegel, 0.033);
        }
        assert!(pegel > 0.25 && pegel < 0.4, "gehalten: {pegel}");
        let mut schreiben = 1.0;
        for _ in 0..30 {
            schreiben = super::repeat_level(schreiben, 0.1);
        }
        assert!(schreiben > pegel && schreiben < 0.8, "schreiben: {schreiben}");
        assert!(super::repeat_level(pegel, 1.0) > 0.99);
        assert!(UiSound::Erase.ducks() && UiSound::Type.ducks() && !UiSound::Click.ducks());
    }
}
