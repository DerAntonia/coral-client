// Server-Resourcepacks: was der Server angeboten hat und wie weit jedes Pack ist.
//
// Ablauf wie bei Vanilla: Angebot (Push) -> "angenommen" melden -> laden -> "heruntergeladen"
// -> alle Packs zusammen mit den eigenen Skins neu laden -> "erfolgreich geladen". Mehrere
// Angebote kurz hintereinander werden in EINEM Neuladen zusammengefasst: es geht erst los,
// wenn kein Download mehr laeuft.
//
// Die Packs liegen nach ihrem SHA-1 benannt in ~/coral-client/server-packs -- beim naechsten
// Besuch desselben Servers wird nichts erneut geladen. Sie gehoeren zur Verbindung: beim
// Trennen fallen sie wieder weg, wie bei Vanilla.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Downloading,
    Ready(PathBuf),
    Failed,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: [u8; 16],
    pub state: State,
    // "Erfolgreich geladen" schon angezeigt -- je Pack nur einmal.
    pub reported: bool,
}

#[derive(Default)]
pub struct ServerPacks {
    // In Angebotsreihenfolge; das zuletzt angebotene liegt oben.
    entries: Vec<Entry>,
    // Die Menge fertiger Packs hat sich geaendert, seit zuletzt neu geladen wurde.
    dirty: bool,
    // Packs, die im gerade aktiven Stapel stecken.
    applied: Vec<[u8; 16]>,
    // Adresse des Servers, der die Packs geschickt hat (s. keys).
    address: String,
}

// Unter welchen Namen die Einstellungen eines Server-Packs gespeichert werden: nach seinem
// SHA-1 (genau dieses Pack, egal von welchem Server) und nach Server-Adresse samt Pack-UUID
// (ueberlebt, wenn der Server sein Pack aendert -- Vanilla-Server leiten die UUID aus der URL
// ab, sie bleibt also). Die Packs liegen nach ihrem SHA-1 benannt im Cache.
pub fn keys(address: &str, id: [u8; 16], path: &std::path::Path) -> (Option<String>, String) {
    let hash = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).filter(|h| h.len() == 40);
    let uuid: String = id.iter().map(|b| format!("{b:02x}")).collect();
    (hash.map(|h| format!("server-pack:{h}")), format!("server:{}:{uuid}", address.trim().to_ascii_lowercase()))
}

// Wo der Server-Eintrag im Stapel dieses Servers gespeichert wird.
pub fn position_key(address: &str) -> String {
    format!("server:{}", address.trim().to_ascii_lowercase())
}

pub fn cache_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| PathBuf::from(h).join("coral-client").join("server-packs"))
}

// Schon geladene Datei zu einem angekuendigten SHA-1, falls vorhanden.
pub fn cached(hash: &str) -> Option<PathBuf> {
    let hash = hash.to_ascii_lowercase();
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let pfad = cache_dir()?.join(format!("{hash}.zip"));
    pfad.is_file().then_some(pfad)
}

// Nur http(s) -- wie Vanilla, das andere Schemata als ungueltige Adresse ablehnt.
pub fn valid_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

impl ServerPacks {
    // Neues Angebot. Ein erneutes Angebot derselben UUID ersetzt das alte (Vanilla ebenso).
    pub fn push(&mut self, id: [u8; 16], state: State) {
        self.entries.retain(|e| e.id != id);
        if matches!(state, State::Ready(_)) {
            self.dirty = true;
        }
        self.entries.push(Entry { id, state, reported: false });
    }

    // Download fertig (oder gescheitert). false, wenn das Pack inzwischen weg ist.
    pub fn finish(&mut self, id: [u8; 16], state: State) -> bool {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id && e.state == State::Downloading) else { return false };
        if matches!(state, State::Ready(_)) {
            self.dirty = true;
        }
        e.state = state;
        true
    }

    // Ein Pack (oder alle) entfernen.
    pub fn remove(&mut self, id: Option<[u8; 16]>) {
        let vorher = self.entries.len();
        self.entries.retain(|e| id.is_some_and(|id| e.id != id));
        if self.entries.len() != vorher && self.applied.iter().any(|a| !self.entries.iter().any(|e| e.id == *a)) {
            self.dirty = true;
        }
    }

    // Neue Verbindung zu dieser Adresse.
    pub fn set_address(&mut self, address: &str) {
        self.address = address.to_string();
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    // Die fertigen Packs, oberstes zuerst -- wie begin_reload, nur ohne etwas umzustellen.
    pub fn ready(&self) -> Vec<([u8; 16], PathBuf)> {
        self.entries
            .iter()
            .rev()
            .filter_map(|e| match &e.state {
                State::Ready(p) => Some((e.id, p.clone())),
                _ => None,
            })
            .collect()
    }

    // Verbindung weg: alles vergessen; liegt etwas im aktiven Stapel, muss neu geladen werden.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.dirty = !self.applied.is_empty();
    }

    // Soll jetzt neu geladen werden? Erst, wenn kein Download mehr laeuft.
    pub fn wants_reload(&self) -> bool {
        self.dirty && !self.entries.iter().any(|e| e.state == State::Downloading)
    }

    // Neuladen beginnt: die fertigen Packs, oberstes zuerst, samt ihren IDs.
    pub fn begin_reload(&mut self) -> Vec<([u8; 16], PathBuf)> {
        self.dirty = false;
        self.ready()
    }

    // Neuladen fertig: welche Packs jetzt "erfolgreich geladen" (bzw. bei Fehler "Neuladen
    // gescheitert") melden sollen -- jedes nur einmal.
    pub fn end_reload(&mut self, ids: &[[u8; 16]], ok: bool) -> Vec<[u8; 16]> {
        self.applied = if ok { ids.to_vec() } else { Vec::new() };
        let mut melden = Vec::new();
        for e in &mut self.entries {
            if ids.contains(&e.id) && !e.reported {
                e.reported = true;
                melden.push(e.id);
            }
        }
        melden
    }

    // Tab-Wechsel: die Packs des alten Tabs nach `alt`, die des neuen aus `neu` hierher. Der
    // geladene Stapel bleibt, bis neu geladen ist; dafuer reicht schon ein Unterschied.
    pub fn switch_tab(&mut self, alt: &mut ServerPacks, neu: &mut ServerPacks) {
        std::mem::swap(&mut self.entries, &mut alt.entries);
        std::mem::swap(&mut self.address, &mut alt.address);
        std::mem::swap(&mut self.entries, &mut neu.entries);
        std::mem::swap(&mut self.address, &mut neu.address);
        let fertig: Vec<[u8; 16]> = self.ready().into_iter().map(|(id, _)| id).collect();
        self.dirty = fertig != self.applied;
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.applied.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bereit(n: u8) -> State {
        State::Ready(PathBuf::from(format!("/p{n}.zip")))
    }

    #[test]
    fn zwei_angebote_ein_neuladen() {
        let mut s = ServerPacks::default();
        s.push([1; 16], State::Downloading);
        s.push([2; 16], State::Downloading);
        assert!(!s.wants_reload());
        assert!(s.finish([1; 16], bereit(1)));
        assert!(!s.wants_reload(), "das zweite laedt noch");
        assert!(s.finish([2; 16], bereit(2)));
        assert!(s.wants_reload());
        let stapel = s.begin_reload();
        // Das zuletzt angebotene liegt oben.
        assert_eq!(stapel.iter().map(|(id, _)| id[0]).collect::<Vec<_>>(), vec![2, 1]);
        let ids: Vec<_> = stapel.iter().map(|(id, _)| *id).collect();
        assert_eq!(s.end_reload(&ids, true).len(), 2);
        assert!(!s.wants_reload());
        // Erneutes Neuladen meldet nichts doppelt.
        assert!(s.end_reload(&ids, true).is_empty());
    }

    #[test]
    fn tabwechsel_laedt_nur_bei_anderen_packs() {
        let mut s = ServerPacks::default();
        s.push([1; 16], bereit(1));
        let ids: Vec<_> = s.begin_reload().iter().map(|(id, _)| *id).collect();
        s.end_reload(&ids, true);
        // Neuer Tab ohne Packs: der geladene Stapel muss weg.
        let (mut alt, mut neu) = (ServerPacks::default(), ServerPacks::default());
        s.switch_tab(&mut alt, &mut neu);
        assert!(s.wants_reload());
        assert_eq!(alt.ready().len(), 1, "die Packs des alten Tabs liegen bei ihm");
        s.end_reload(&[], true);
        // Zurueck: dieselben Packs wie vorher kommen wieder, also neu laden.
        let mut leer = ServerPacks::default();
        s.switch_tab(&mut leer, &mut alt);
        assert!(s.wants_reload());
        s.end_reload(&ids, true);
        // Zwei Tabs mit gleichen Packs: kein Neuladen.
        let mut gleich = ServerPacks::default();
        gleich.push([1; 16], bereit(1));
        let mut ablage = ServerPacks::default();
        s.switch_tab(&mut ablage, &mut gleich);
        assert!(!s.wants_reload());
    }

    #[test]
    fn trennen_laedt_ohne_serverpacks_neu() {
        let mut s = ServerPacks::default();
        s.push([1; 16], bereit(1));
        let ids: Vec<_> = s.begin_reload().iter().map(|(id, _)| *id).collect();
        s.end_reload(&ids, true);
        s.clear();
        assert!(s.wants_reload());
        assert!(s.begin_reload().is_empty());
        s.end_reload(&[], true);
        assert!(s.is_empty());
        // Nie angewendet: Trennen loest nichts aus.
        let mut t = ServerPacks::default();
        t.push([3; 16], State::Downloading);
        t.clear();
        assert!(!t.wants_reload());
    }

    #[test]
    fn entfernen_und_spaeter_download() {
        let mut s = ServerPacks::default();
        s.push([1; 16], State::Downloading);
        s.remove(Some([1; 16]));
        assert!(!s.finish([1; 16], bereit(1)), "zu spaet fertig: verworfen");
        assert!(!valid_url("ftp://x"));
        assert!(valid_url("https://pack.example/p.zip"));
        assert!(cached("kein-hash").is_none());
    }

    #[test]
    fn schluessel_nach_hash_und_adresse() {
        let h = "a".repeat(40);
        let (hash, adresse) = keys(" Play.Example.net ", [0xab; 16], &PathBuf::from(format!("/x/{h}.zip")));
        assert_eq!(hash.as_deref(), Some(format!("server-pack:{h}").as_str()));
        assert_eq!(adresse, format!("server:play.example.net:{}", "ab".repeat(16)));
        assert_eq!(keys("x", [0; 16], &PathBuf::from("/x/kurz.zip")).0, None);
        assert_eq!(position_key("Play.Example.net"), "server:play.example.net");
    }
}
