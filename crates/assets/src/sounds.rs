//! `sounds.json`: welche Audiodateien zu einem Sound-Event gehoeren.
//!
//! Das Sound-Paket vom Server nennt nur einen Event-Namen wie `block.stone.step`. Welche
//! Datei dazu gehoert, steht nicht im Protokoll, sondern in `sounds.json` des Resourcepacks --
//! und dort sind es meist MEHRERE: `block.stone.step` hat sechs Aufnahmen, aus denen bei
//! jedem Schritt eine gewaehlt wird. Ohne das klaenge jeder Schritt exakt gleich.
//!
//! Die Datei liegt im geteilten Objects-Store der Installation -- mitgeliefert wird nichts.
//! Geladen wird sie ueber `MinecraftInstallation::load_layers` und NICHT ueber `load_bytes`:
//! sounds.json ist eine LISTE, zu der jede Ebene beitraegt, keine Datei, von der es genau eine
//! gibt (s. `merge_layer` und den Kommentar an `load_layers`).

use std::collections::HashMap;

use serde::Deserialize;

use crate::installation::{AssetsError, MinecraftInstallation};

/// Eine einzelne Aufnahme hinter einem Sound-Event.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundVariant {
    /// Fertiger Pfad fuers Laden, z.B. "minecraft/sounds/step/stone1.ogg".
    pub path: String,
    /// Lautstaerkefaktor DIESER Aufnahme (aus sounds.json), unabhaengig von der Lautstaerke
    /// im Paket -- manche Varianten eines Events sind von Haus aus leiser abgemischt.
    pub volume: f32,
    /// Tonhoehenfaktor dieser Aufnahme, ebenfalls unabhaengig vom Paket.
    pub pitch: f32,
    /// Relatives Gewicht bei der Auswahl. Vanilla nennt eine Variante mit `weight: 2` doppelt
    /// so oft wie eine mit 1 -- so klingt eine haeufige Variante haeufig und eine seltene
    /// selten, statt alle gleich oft.
    pub weight: u32,
}

/// Alle Sound-Events der Installation.
pub struct SoundIndex {
    events: HashMap<String, Vec<SoundVariant>>,
}

// --- Rohform von sounds.json -------------------------------------------------------------

#[derive(Deserialize)]
struct RawEvent {
    #[serde(default)]
    sounds: Vec<RawEntry>,
    // "Wirf die Liste der tieferen Ebenen fuer dieses Event weg." Ohne das Flag ERGAENZT ein
    // Resourcepack Vanillas Aufnahmen, statt sie zu ersetzen -- man hoerte dann beide.
    #[serde(default)]
    replace: bool,
}

// Ein Eintrag ist entweder nur ein Dateiname oder ein Objekt mit Zusatzangaben. Beide Formen
// kommen in Vanilla vor (4847-mal die kurze, 2581-mal die lange).
#[derive(Deserialize)]
#[serde(untagged)]
enum RawEntry {
    Name(String),
    Detail(RawDetail),
}

#[derive(Deserialize)]
struct RawDetail {
    name: String,
    #[serde(default = "eins")]
    volume: f32,
    #[serde(default = "eins")]
    pitch: f32,
    #[serde(default = "ein_gewicht")]
    weight: u32,
    // "sound" (Standard) = `name` ist eine Datei. "event" = `name` ist ein ANDERES
    // Sound-Event, dessen Varianten hier einzusetzen sind (52-mal in Vanilla, z.B. zeigt
    // block.cave_vines.pick_berries auf block.sweet_berry_bush.pick_berries).
    #[serde(default = "sound_typ")]
    r#type: String,
}

fn eins() -> f32 {
    1.0
}

fn ein_gewicht() -> u32 {
    1
}

fn sound_typ() -> String {
    "sound".to_string()
}

/// Wo `sounds.json` im Asset-Baum liegt.
const SOUNDS_JSON: &str = "minecraft/sounds.json";

impl SoundIndex {
    /// Alle Ebenen laden und zusammenfuehren -- Vanilla als Grundlage, Resourcepacks darueber.
    ///
    /// Eine kaputte Ebene wirft nicht alles weg: sie wird uebersprungen, die anderen zaehlen
    /// weiter. Ein Pack mit fehlerhafter sounds.json soll den Ton nicht komplett abschalten.
    pub fn load(installation: &mut MinecraftInstallation) -> Result<SoundIndex, AssetsError> {
        let ebenen = installation.load_layers(SOUNDS_JSON);
        if ebenen.is_empty() {
            return Err(AssetsError::AssetNotFound { virtual_path: SOUNDS_JSON.to_string() });
        }
        let mut zusammen: HashMap<String, RawEvent> = HashMap::new();
        for bytes in &ebenen {
            match serde_json::from_slice::<HashMap<String, RawEvent>>(bytes) {
                Ok(ebene) => merge_layer(&mut zusammen, ebene),
                Err(e) => eprintln!("[assets] eine sounds.json-Ebene ist fehlerhaft und wird uebersprungen: {e}"),
            }
        }
        if zusammen.is_empty() {
            return Err(AssetsError::AssetNotFound { virtual_path: SOUNDS_JSON.to_string() });
        }
        Ok(SoundIndex { events: flatten(&zusammen) })
    }

    /// Alle Aufnahmen eines Events. `None` = Event unbekannt.
    pub fn variants(&self, event: &str) -> Option<&[SoundVariant]> {
        self.events.get(strip_namespace(event)).map(|v| v.as_slice())
    }

    /// Alle bekannten Event-Namen (ohne Namensraum). Fuer Pruefungen und Werkzeuge.
    pub fn event_names(&self) -> impl Iterator<Item = &str> {
        self.events.keys().map(|k| k.as_str())
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Eine Aufnahme auswaehlen, gesteuert von der Zufallssaat aus dem Paket.
    ///
    /// Die Saat kommt vom SERVER, damit alle Spieler dieselbe Variante hoeren -- wer neben dir
    /// steht, soll denselben Schritt hoeren wie du. Genau DIESE Auswahl ist bewusst nicht
    /// bitgleich mit Vanilla: der Client muesste dafuer Minecrafts Zufallsgenerator
    /// nachbauen. Der hoerbare Unterschied waere, ob `stone1` oder `stone3` erklingt --
    /// derselbe Schritt, andere Aufnahme. Deterministisch ist es trotzdem: dieselbe Saat
    /// ergibt hier immer dieselbe Variante.
    pub fn pick(&self, event: &str, seed: i64) -> Option<&SoundVariant> {
        let varianten = self.variants(event)?;
        let gesamt: u32 = varianten.iter().map(|v| v.weight.max(1)).sum();
        if gesamt == 0 {
            return varianten.first();
        }
        // Die unteren Bits einer Saat sind bei vielen Generatoren schwach; das Mischen
        // (Splitmix64-Finalisierer) verteilt sie ueber das ganze Wort, bevor der Rest
        // gebildet wird. Ohne das waeren bei aufeinanderfolgenden Saaten immer dieselben
        // paar Varianten dran.
        let mut x = seed as u64;
        x ^= x >> 30;
        x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^= x >> 31;
        let mut wahl = (x % gesamt as u64) as u32;
        for variante in varianten {
            let gewicht = variante.weight.max(1);
            if wahl < gewicht {
                return Some(variante);
            }
            wahl -= gewicht;
        }
        varianten.last()
    }
}

/// Eine hoehere Ebene in die bisherigen Eintraege einarbeiten.
///
/// Genau Vanillas Regel aus `SoundManager.SoundList.register`, am Bytecode abgelesen: gibt es
/// das Event noch nicht ODER steht `replace` auf true, faengt eine neue Liste an -- sonst
/// werden die Aufnahmen der hoeheren Ebene an die vorhandenen ANGEHAENGT.
///
/// Deshalb setzt ein Texturpaket, das Vanillas Schritte ersetzen will, `replace: true`: ohne
/// das Flag klaenge bei jedem Schritt mal seine, mal Vanillas Aufnahme.
fn merge_layer(bisher: &mut HashMap<String, RawEvent>, ebene: HashMap<String, RawEvent>) {
    for (name, mut event) in ebene {
        match bisher.get_mut(&name) {
            Some(vorhanden) if !event.replace => vorhanden.sounds.append(&mut event.sounds),
            _ => {
                bisher.insert(name, event);
            }
        }
    }
}

/// `minecraft:block.stone.step` -> `block.stone.step`. In sounds.json stehen die Events ohne
/// Namensraum; im Paket kommen sie mit.
fn strip_namespace(event: &str) -> &str {
    event.split_once(':').map(|(_, rest)| rest).unwrap_or(event)
}

/// Aus der Rohform die fertigen Variantenlisten bauen -- inklusive Aufloesen der
/// `type: "event"`-Verweise.
fn flatten(roh: &HashMap<String, RawEvent>) -> HashMap<String, Vec<SoundVariant>> {
    let mut fertig = HashMap::with_capacity(roh.len());
    for name in roh.keys() {
        let mut besucht = Vec::new();
        let varianten = aufloesen(roh, name, &mut besucht);
        fertig.insert(name.clone(), varianten);
    }
    fertig
}

// Rekursiv, weil ein Verweis wieder auf ein Event zeigen kann. `besucht` bricht Zyklen ab:
// ein fehlerhaftes Resourcepack darf den Client nicht in eine Endlosschleife schicken.
fn aufloesen(roh: &HashMap<String, RawEvent>, name: &str, besucht: &mut Vec<String>) -> Vec<SoundVariant> {
    if besucht.iter().any(|b| b == name) {
        return Vec::new();
    }
    besucht.push(name.to_string());
    let Some(event) = roh.get(name) else { return Vec::new() };

    let mut out = Vec::new();
    for eintrag in &event.sounds {
        match eintrag {
            RawEntry::Name(datei) => out.push(SoundVariant { path: asset_path(datei), volume: 1.0, pitch: 1.0, weight: 1 }),
            RawEntry::Detail(d) if d.r#type == "event" => {
                // Die Angaben des VERWEISES multiplizieren sich auf die eingesetzten
                // Varianten: ein Verweis mit volume 0.5 macht das ganze fremde Event leiser.
                for mut variante in aufloesen(roh, strip_namespace(&d.name), besucht) {
                    variante.volume *= d.volume;
                    variante.pitch *= d.pitch;
                    variante.weight = variante.weight.max(1) * d.weight.max(1);
                    out.push(variante);
                }
            }
            RawEntry::Detail(d) => out.push(SoundVariant { path: asset_path(&d.name), volume: d.volume, pitch: d.pitch, weight: d.weight }),
        }
    }
    besucht.pop();
    out
}

/// `step/stone1` -> `minecraft/sounds/step/stone1.ogg`, `namensraum:pfad` entsprechend.
fn asset_path(name: &str) -> String {
    match name.split_once(':') {
        Some((namensraum, pfad)) => format!("{namensraum}/sounds/{pfad}.ogg"),
        None => format!("minecraft/sounds/{name}.ogg"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> HashMap<String, Vec<SoundVariant>> {
        flatten(&serde_json::from_str(json).unwrap())
    }

    // Ebenen zusammenfuehren, wie SoundIndex::load es tut: unterste zuerst.
    fn schichten(ebenen: &[&str]) -> HashMap<String, Vec<SoundVariant>> {
        let mut zusammen: HashMap<String, RawEvent> = HashMap::new();
        for json in ebenen {
            merge_layer(&mut zusammen, serde_json::from_str(json).unwrap());
        }
        flatten(&zusammen)
    }

    // DER Bug, der die Weltgeraeusche stumm gemacht hat: ein Resourcepack mit eigener
    // sounds.json (19 Eintraege) verdeckte Vanillas 1837, weil nur die oberste Ebene gelesen
    // wurde. Vanillas uebrige Eintraege MUESSEN erhalten bleiben.
    #[test]
    fn ein_pack_verdeckt_vanillas_uebrige_klaenge_nicht() {
        let e = schichten(&[
            r#"{"block.stone.step":{"sounds":["step/stone1"]},
                "entity.player.hurt":{"sounds":["damage/hit1"]},
                "entity.generic.explode":{"sounds":["random/explode1"]}}"#,
            // Das Pack kennt nur EINEN Klang.
            r#"{"block.stone.step":{"sounds":["custom/step"],"replace":true}}"#,
        ]);
        assert_eq!(e.len(), 3, "Vanillas uebrige Events muessen bleiben");
        assert_eq!(e["entity.player.hurt"][0].path, "minecraft/sounds/damage/hit1.ogg");
        // Und das ersetzte Event ist wirklich ersetzt, nicht ergaenzt.
        assert_eq!(e["block.stone.step"].len(), 1);
        assert_eq!(e["block.stone.step"][0].path, "minecraft/sounds/custom/step.ogg");
    }

    // Ohne `replace` ERGAENZT ein Pack -- genau deshalb setzen Packs das Flag.
    #[test]
    fn ohne_replace_haengt_ein_pack_seine_aufnahmen_an() {
        let e = schichten(&[
            r#"{"block.stone.step":{"sounds":["step/stone1","step/stone2"]}}"#,
            r#"{"block.stone.step":{"sounds":["custom/step"]}}"#,
        ]);
        assert_eq!(e["block.stone.step"].len(), 3, "Vanillas zwei plus die eine des Packs");
        assert_eq!(e["block.stone.step"][2].path, "minecraft/sounds/custom/step.ogg");
    }

    // Ein Pack darf auch Events HINZUFUEGEN, die es in Vanilla nicht gibt.
    #[test]
    fn ein_pack_darf_neue_events_beisteuern() {
        let e = schichten(&[r#"{"a":{"sounds":["a1"]}}"#, r#"{"b":{"sounds":["b1"]}}"#]);
        assert_eq!(e.len(), 2);
        assert_eq!(e["b"][0].path, "minecraft/sounds/b1.ogg");
    }

    // Die kurze Form (nur ein Dateiname) ist mit Abstand die haeufigste in Vanilla.
    #[test]
    fn die_kurze_form_wird_zu_einem_asset_pfad() {
        let e = parse(r#"{"block.stone.step":{"sounds":["step/stone1","step/stone2"]}}"#);
        let v = &e["block.stone.step"];
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].path, "minecraft/sounds/step/stone1.ogg");
        assert_eq!(v[0].volume, 1.0);
        assert_eq!(v[0].weight, 1);
    }

    #[test]
    fn die_lange_form_traegt_lautstaerke_und_tonhoehe() {
        let e = parse(r#"{"a":{"sounds":[{"name":"x/y","volume":0.6,"pitch":1.2,"weight":3}]}}"#);
        let v = &e["a"][0];
        assert_eq!(v.path, "minecraft/sounds/x/y.ogg");
        assert_eq!((v.volume, v.pitch, v.weight), (0.6, 1.2, 3));
    }

    // Felder, die uns nicht interessieren (stream, attenuation_distance, subtitle), duerfen
    // das Parsen nicht kippen -- sonst faellt bei der naechsten Version, die ein Feld
    // hinzufuegt, die ganze Tonausgabe aus.
    #[test]
    fn unbekannte_felder_stoeren_nicht() {
        let e = parse(r#"{"a":{"subtitle":"s","sounds":[{"name":"x","stream":true,"attenuation_distance":16,"neu":42}]}}"#);
        assert_eq!(e["a"].len(), 1);
    }

    // 52-mal in Vanilla: ein Event zeigt auf ein anderes.
    #[test]
    fn ein_verweis_auf_ein_anderes_event_wird_eingesetzt() {
        let e = parse(
            r#"{"quelle":{"sounds":["a1","a2"]},
                "zeiger":{"sounds":[{"name":"quelle","type":"event"}]}}"#,
        );
        assert_eq!(e["zeiger"].len(), 2);
        assert_eq!(e["zeiger"][0].path, "minecraft/sounds/a1.ogg");
    }

    #[test]
    fn ein_verweis_multipliziert_seine_eigene_lautstaerke_darauf() {
        let e = parse(
            r#"{"quelle":{"sounds":[{"name":"a1","volume":0.5}]},
                "zeiger":{"sounds":[{"name":"quelle","type":"event","volume":0.5}]}}"#,
        );
        assert_eq!(e["zeiger"][0].volume, 0.25);
    }

    // Ein fehlerhaftes Resourcepack darf den Client nicht aufhaengen.
    #[test]
    fn ein_ringverweis_endet_statt_ewig_zu_laufen() {
        let e = parse(
            r#"{"a":{"sounds":[{"name":"b","type":"event"}]},
                "b":{"sounds":[{"name":"a","type":"event"}]}}"#,
        );
        assert!(e["a"].is_empty());
        assert!(e["b"].is_empty());
    }

    // Ein Verweis ins Leere ist kein Absturz, sondern ein Event ohne Aufnahmen.
    #[test]
    fn ein_verweis_auf_ein_fehlendes_event_bleibt_leer() {
        let e = parse(r#"{"a":{"sounds":[{"name":"gibtsnicht","type":"event"}]}}"#);
        assert!(e["a"].is_empty());
    }

    fn index(json: &str) -> SoundIndex {
        SoundIndex { events: parse(json) }
    }

    // Der Namensraum aus dem Paket muss abfallen -- sonst wird KEIN einziges Event gefunden.
    #[test]
    fn der_namensraum_aus_dem_paket_stoert_die_suche_nicht() {
        let i = index(r#"{"block.stone.step":{"sounds":["step/stone1"]}}"#);
        assert!(i.variants("minecraft:block.stone.step").is_some());
        assert!(i.variants("block.stone.step").is_some());
        assert!(i.variants("minecraft:gibtsnicht").is_none());
    }

    // Dieselbe Saat, dieselbe Variante -- daran haengt, dass zwei Spieler dasselbe hoeren.
    #[test]
    fn dieselbe_saat_waehlt_immer_dieselbe_variante() {
        let i = index(r#"{"a":{"sounds":["s1","s2","s3","s4","s5","s6"]}}"#);
        for saat in [0i64, 1, -7, i64::MAX, i64::MIN] {
            assert_eq!(i.pick("a", saat), i.pick("a", saat));
        }
    }

    // ... und verschiedene Saaten muessen wirklich streuen, sonst klaenge jeder Schritt gleich
    // und die sechs Aufnahmen waeren umsonst.
    #[test]
    fn verschiedene_saaten_treffen_alle_varianten() {
        let i = index(r#"{"a":{"sounds":["s1","s2","s3","s4","s5","s6"]}}"#);
        let getroffen: std::collections::HashSet<_> = (0..2000).map(|s| i.pick("a", s).unwrap().path.clone()).collect();
        assert_eq!(getroffen.len(), 6, "nicht alle sechs Aufnahmen kamen vor");
    }

    // Gewichte muessen wirken: eine Variante mit weight 9 muss deutlich haeufiger drankommen
    // als eine mit weight 1.
    #[test]
    fn gewichte_verschieben_die_haeufigkeit() {
        let i = index(r#"{"a":{"sounds":[{"name":"selten","weight":1},{"name":"haeufig","weight":9}]}}"#);
        let haeufig = (0..10_000).filter(|s| i.pick("a", *s).unwrap().path.contains("haeufig")).count();
        assert!((8200..9800).contains(&haeufig), "haeufige Variante kam {haeufig} von 10000 Mal");
    }

    #[test]
    fn ein_unbekanntes_event_liefert_nichts() {
        let i = index(r#"{"a":{"sounds":["s1"]}}"#);
        assert_eq!(i.pick("b", 0), None);
    }

    // Ein Event ohne Aufnahmen (z.B. nach einem ins Leere zeigenden Verweis) darf nicht
    // durch null teilen.
    #[test]
    fn ein_leeres_event_teilt_nicht_durch_null() {
        let i = index(r#"{"a":{"sounds":[]}}"#);
        assert_eq!(i.pick("a", 42), None);
    }
}
