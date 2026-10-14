// Gespeicherte Konten des Acc-Managers: jedes Konto, mit dem man sich einmal angemeldet hat
// (Microsoft, Refresh-Token, eingefuegtes Minecraft-Token), dazu die Konten aus Prism. Liegt
// fuer alle Configs gemeinsam in accounts.conf -- ein Konto gehoert dem Rechner, nicht einer
// Config. Welches gerade angemeldet ist, steht weiter in den Settings (account_uuid).
//
// Die Datei enthaelt Tokens, deshalb (unter Unix) nur fuer den Besitzer lesbar.

use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    // Microsoft-Anmeldung oder eingeloestes Refresh-Token: erneuert sich selbst.
    Microsoft,
    // Eingefuegtes Minecraft-Token: gilt bis zu seinem Ablauf (~24 h), danach ist Schluss.
    Token,
    // Aus Prism uebernommen: Prism erneuert das Token, wir lesen es bei jedem Oeffnen neu.
    Prism,
}

impl Source {
    fn key(self) -> &'static str {
        match self {
            Source::Microsoft => "microsoft",
            Source::Token => "token",
            Source::Prism => "prism",
        }
    }

    fn from_key(key: &str) -> Option<Source> {
        match key {
            "microsoft" => Some(Source::Microsoft),
            "token" => Some(Source::Token),
            "prism" => Some(Source::Prism),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub name: String,
    // UUID als 32 Hex-Zeichen ohne Bindestriche (wie auth::Credentials::profile_id).
    pub uuid: String,
    pub source: Source,
    // Minecraft-Access-Token.
    pub access_token: String,
    // Nur bei Source::Microsoft: Refresh-Token und die Client-ID der App, die es ausgestellt
    // hat -- Microsoft nimmt es mit keiner anderen an.
    pub refresh_token: String,
    pub client_id: String,
}

impl Account {
    pub fn credentials(&self) -> auth::Credentials {
        auth::Credentials { access_token: self.access_token.clone(), profile_id: self.uuid.clone(), username: self.name.clone() }
    }
}

// Ergebnis der letzten Pruefung (nicht gespeichert, beim Start ist alles Unknown).
#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    Unknown,
    Checking,
    Valid,
    Invalid(String),
}

#[derive(Default)]
pub struct Store {
    pub list: Vec<Account>,
    // Vergessene Prism-Konten: tauchen beim naechsten Einlesen nicht wieder auf, erst nach
    // "Aus Prism importieren".
    pub forgotten: HashSet<String>,
}

// Vergleichsschluessel einer UUID: klein, ohne Bindestriche (Prism schreibt sie mit).
pub fn key(uuid: &str) -> String {
    uuid.replace('-', "").to_lowercase()
}

fn same_uuid(a: &str, b: &str) -> bool {
    key(a) == key(b)
}

impl Store {
    // Format: eine Zeile je Konto "konto<TAB>quelle<TAB>name<TAB>uuid<TAB>token<TAB>refresh<TAB>
    // client-id", dazu "vergessen<TAB>uuid". Tokens und Namen enthalten keine Tabs.
    pub fn parse(text: &str) -> Store {
        let mut store = Store::default();
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            match f.as_slice() {
                ["konto", quelle, name, uuid, token, refresh, client_id] => {
                    let Some(source) = Source::from_key(quelle) else { continue };
                    if uuid.is_empty() || token.is_empty() {
                        continue;
                    }
                    store.list.push(Account {
                        name: name.to_string(),
                        uuid: uuid.to_string(),
                        source,
                        access_token: token.to_string(),
                        refresh_token: refresh.to_string(),
                        client_id: client_id.to_string(),
                    });
                }
                ["vergessen", uuid] => {
                    store.forgotten.insert(key(uuid));
                }
                _ => {}
            }
        }
        store
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for a in &self.list {
            let feld = |s: &str| s.replace(['\t', '\n', '\r'], "");
            out.push_str(&format!(
                "konto\t{}\t{}\t{}\t{}\t{}\t{}\n",
                a.source.key(),
                feld(&a.name),
                feld(&a.uuid),
                feld(&a.access_token),
                feld(&a.refresh_token),
                feld(&a.client_id)
            ));
        }
        let mut vergessen: Vec<&String> = self.forgotten.iter().collect();
        vergessen.sort();
        for uuid in vergessen {
            out.push_str(&format!("vergessen\t{uuid}\n"));
        }
        out
    }

    pub fn find(&self, uuid: &str) -> Option<usize> {
        self.list.iter().position(|a| same_uuid(&a.uuid, uuid))
    }

    // Nach einer Anmeldung: Konto anlegen oder auffrischen (Name kann sich geaendert haben).
    // Mit Refresh-Token wird es ein Microsoft-Konto; ohne behaelt ein vorhandenes seine Quelle
    // und sein Refresh-Token.
    pub fn upsert(&mut self, creds: &auth::Credentials, msa: Option<(String, String)>, source_without_msa: Source) {
        self.forgotten.remove(&key(&creds.profile_id));
        let neu = match self.find(&creds.profile_id) {
            Some(i) => &mut self.list[i],
            None => {
                self.list.push(Account {
                    name: String::new(),
                    uuid: creds.profile_id.clone(),
                    source: source_without_msa,
                    access_token: String::new(),
                    refresh_token: String::new(),
                    client_id: String::new(),
                });
                self.list.last_mut().unwrap()
            }
        };
        neu.name = creds.username.clone();
        neu.access_token = creds.access_token.clone();
        if let Some((refresh, client_id)) = msa {
            neu.source = Source::Microsoft;
            neu.refresh_token = refresh;
            neu.client_id = client_id;
        }
    }

    pub fn forget(&mut self, i: usize) -> Option<Account> {
        if i >= self.list.len() {
            return None;
        }
        let a = self.list.remove(i);
        if a.source == Source::Prism {
            self.forgotten.insert(key(&a.uuid));
        }
        Some(a)
    }

    // Prism-Konten einarbeiten. Neue kommen hinten dazu (ausser vergessene, es sei denn
    // `explicit` -- der Knopf "Aus Prism importieren" holt auch die zurueck). Bei einem
    // bekannten Konto ohne eigenes Refresh-Token gewinnt das laenger gueltige Token: Prism
    // erneuert seins, ein eingefuegtes Token laeuft irgendwann ab. Gibt die Zahl neuer
    // Konten zurueck.
    pub fn merge_prism(&mut self, prism: &[auth::LauncherAccount], explicit: bool, now: u64) -> usize {
        let mut neu = 0;
        for p in prism {
            let k = key(&p.uuid);
            if explicit {
                self.forgotten.remove(&k);
            }
            match self.find(&p.uuid) {
                Some(i) => {
                    let a = &mut self.list[i];
                    if a.source != Source::Microsoft && a.access_token != p.token && expiry(&p.token).unwrap_or(now) >= expiry(&a.access_token).unwrap_or(0) {
                        a.access_token = p.token.clone();
                        a.name = p.name.clone();
                    }
                }
                None if self.forgotten.contains(&k) => {}
                None => {
                    self.list.push(Account {
                        name: p.name.clone(),
                        uuid: p.uuid.replace('-', ""),
                        source: Source::Prism,
                        access_token: p.token.clone(),
                        refresh_token: String::new(),
                        client_id: String::new(),
                    });
                    neu += 1;
                }
            }
        }
        neu
    }
}

// Ablauf eines Minecraft-Access-Tokens (Unix-Sekunden). Es ist ein JWT: der mittlere Teil ist
// Base64url-JSON mit "exp". None = kein JWT oder kein exp.
pub fn expiry(token: &str) -> Option<u64> {
    use base64::Engine;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp")?.as_u64()
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

// Restlaufzeit fuer die Anzeige: "noch 23 h", "noch 41 min", "abgelaufen".
pub fn remaining_text(exp: u64, now: u64) -> String {
    let rest = exp.saturating_sub(now);
    if rest == 0 {
        "abgelaufen".to_string()
    } else if rest >= 2 * 86_400 {
        format!("noch {} Tage", rest / 86_400)
    } else if rest >= 3600 {
        format!("noch {} h", rest / 3600)
    } else {
        format!("noch {} min", (rest / 60).max(1))
    }
}

// Reihenfolge in Liste und Acc-Manager: das angemeldete Konto zuerst, dann gueltige, gerade
// gepruefte und ungepruefte in gespeicherter Reihenfolge, ungueltige zuletzt.
pub fn display_order(list: &[Account], check: impl Fn(&Account) -> Check, active_uuid: Option<&str>) -> Vec<usize> {
    let rang = |a: &Account| {
        if active_uuid.is_some_and(|u| same_uuid(u, &a.uuid)) {
            0
        } else if matches!(check(a), Check::Invalid(_)) {
            2
        } else {
            1
        }
    };
    let mut order: Vec<usize> = (0..list.len()).collect();
    order.sort_by_key(|&i| rang(&list[i]));
    order
}

pub fn path() -> Option<std::path::PathBuf> {
    assets::config_dir().map(|d| d.join("accounts.conf"))
}

pub fn load() -> Option<Store> {
    let text = std::fs::read_to_string(path()?).ok()?;
    Some(Store::parse(&text))
}

pub fn save(store: &Store) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "kein Konfigurationsverzeichnis"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, store.serialize())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(exp: u64) -> String {
        use base64::Engine;
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!("{}.{}.sig", e.encode(r#"{"alg":"HS256"}"#), e.encode(format!(r#"{{"xuid":"1","exp":{exp}}}"#)))
    }

    fn creds(name: &str, uuid: &str, token: &str) -> auth::Credentials {
        auth::Credentials { access_token: token.into(), profile_id: uuid.into(), username: name.into() }
    }

    fn prism(name: &str, uuid: &str, token: &str) -> auth::LauncherAccount {
        auth::LauncherAccount { name: name.into(), uuid: uuid.into(), token: token.into() }
    }

    #[test]
    fn ablauf_aus_dem_jwt() {
        assert_eq!(expiry(&jwt(1_800_000_000)), Some(1_800_000_000));
        assert_eq!(expiry("kein-jwt"), None);
        assert_eq!(remaining_text(1000, 2000), "abgelaufen");
        assert_eq!(remaining_text(1000 + 23 * 3600 + 59, 1000), "noch 23 h");
        assert_eq!(remaining_text(1000 + 41 * 60, 1000), "noch 41 min");
        assert_eq!(remaining_text(1000 + 30, 1000), "noch 1 min");
        assert_eq!(remaining_text(1000 + 3 * 86_400, 1000), "noch 3 Tage");
    }

    #[test]
    fn speichern_und_laden_ergibt_dasselbe() {
        let mut s = Store::default();
        s.upsert(&creds("DerAntonia", "aa", "t1"), Some(("r1".into(), "cid".into())), Source::Token);
        s.upsert(&creds("uhze", "bb", "t2"), None, Source::Token);
        s.forgotten.insert("cc".into());
        let t = Store::parse(&s.serialize());
        assert_eq!(t.list, s.list);
        assert_eq!(t.forgotten, s.forgotten);
        assert_eq!(t.list[0].source, Source::Microsoft);
        assert_eq!(t.list[1].source, Source::Token);
    }

    #[test]
    fn anmeldung_frischt_ein_bekanntes_konto_auf() {
        let mut s = Store::default();
        s.upsert(&creds("alt", "aa", "t1"), Some(("r1".into(), "cid".into())), Source::Token);
        // Ein spaeteres Token ohne Refresh behaelt Quelle und Refresh-Token.
        s.upsert(&creds("neu", "AA", "t2"), None, Source::Token);
        assert_eq!(s.list.len(), 1);
        assert_eq!((s.list[0].name.as_str(), s.list[0].access_token.as_str(), s.list[0].refresh_token.as_str()), ("neu", "t2", "r1"));
        assert_eq!(s.list[0].source, Source::Microsoft);
    }

    #[test]
    fn prism_konten_kommen_dazu_vergessene_nur_beim_import() {
        let mut s = Store::default();
        let p = [prism("uhze", "11112222-3333-4444-5555-666677778888", &jwt(2000))];
        assert_eq!(s.merge_prism(&p, false, 1000), 1);
        assert_eq!(s.list[0].uuid, "11112222333344445555666677778888");
        assert_eq!(s.merge_prism(&p, false, 1000), 0, "kein Doppel");
        s.forget(0);
        assert_eq!(s.merge_prism(&p, false, 1000), 0, "vergessen bleibt vergessen");
        assert_eq!(s.merge_prism(&p, true, 1000), 1, "Import holt es zurueck");
    }

    #[test]
    fn das_laenger_gueltige_token_gewinnt() {
        let mut s = Store::default();
        s.upsert(&creds("uhze", "aa", &jwt(5000)), None, Source::Token);
        s.merge_prism(&[prism("uhze", "aa", &jwt(3000))], false, 1000);
        assert_eq!(expiry(&s.list[0].access_token), Some(5000));
        s.merge_prism(&[prism("uhze", "aa", &jwt(9000))], false, 1000);
        assert_eq!(expiry(&s.list[0].access_token), Some(9000));
        // Ein Microsoft-Konto erneuert sich selbst, Prism ueberschreibt es nicht.
        s.upsert(&creds("uhze", "aa", "eigenes"), Some(("r".into(), "c".into())), Source::Token);
        s.merge_prism(&[prism("uhze", "aa", &jwt(99_000))], false, 1000);
        assert_eq!(s.list[0].access_token, "eigenes");
    }

    #[test]
    fn angemeldetes_zuerst_ungueltige_zuletzt() {
        let mut s = Store::default();
        for (n, u) in [("a", "1"), ("b", "2"), ("c", "3"), ("d", "4")] {
            s.upsert(&creds(n, u, "t"), None, Source::Token);
        }
        let check = |a: &Account| if a.uuid == "2" { Check::Invalid("x".into()) } else { Check::Valid };
        assert_eq!(display_order(&s.list, check, Some("3")), vec![2, 0, 3, 1]);
    }
}
