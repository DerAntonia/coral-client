// Welche Links ohne Rueckfrage aufgehen: Host aus der URL holen und gegen link_whitelist.txt
// pruefen. Alles andere bekommt vorher ein Bestaetigungsfenster (s. App::open_link).

use std::collections::HashSet;
use std::sync::OnceLock;

struct Liste {
    erlaubt: HashSet<&'static str>,
    ausgenommen: HashSet<&'static str>,
}

fn liste() -> &'static Liste {
    static LISTE: OnceLock<Liste> = OnceLock::new();
    LISTE.get_or_init(|| {
        let mut l = Liste { erlaubt: HashSet::new(), ausgenommen: HashSet::new() };
        for zeile in include_str!("link_whitelist.txt").lines().map(str::trim) {
            if zeile.is_empty() || zeile.starts_with('#') {
                continue;
            }
            match zeile.strip_prefix('!') {
                Some(x) => l.ausgenommen.insert(x),
                None => l.erlaubt.insert(zeile),
            };
        }
        l
    })
}

// Host einer http(s)-URL, klein und ohne Port, Anmeldedaten und Schlusspunkt. Nach dem letzten
// '@' vor dem Pfad, sonst gaelte "https://google.com@boese.de" als google.com.
pub fn host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let autoritaet = rest.split(['/', '?', '#', '\\']).next()?;
    let ohne_nutzer = autoritaet.rsplit('@').next()?;
    let host = if ohne_nutzer.starts_with('[') {
        ohne_nutzer.split(']').next().map(|h| format!("{h}]"))?
    } else {
        ohne_nutzer.split(':').next()?.to_string()
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

// Der Host selbst oder eine seiner Elterndomains, z.B. "www.youtube.com" -> "youtube.com".
fn domain_stufen(host: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(host), |h| h.split_once('.').map(|(_, r)| r))
}

pub fn trusted(url: &str) -> bool {
    let Some(host) = host(url) else { return false };
    let l = liste();
    let ausgenommen = domain_stufen(&host).any(|d| l.ausgenommen.contains(d));
    let erlaubt = domain_stufen(&host).any(|d| l.erlaubt.contains(d));
    erlaubt && !ausgenommen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_aus_url() {
        assert_eq!(host("https://www.YouTube.com/watch?v=1").as_deref(), Some("www.youtube.com"));
        assert_eq!(host("http://example.org:8080").as_deref(), Some("example.org"));
        assert_eq!(host("https://google.com@boese.de/x").as_deref(), Some("boese.de"));
        assert_eq!(host("https://discord.gg./abc").as_deref(), Some("discord.gg"));
        assert_eq!(host("ftp://x.y"), None);
    }

    #[test]
    fn bekannte_seiten_ohne_rueckfrage() {
        assert!(trusted("https://www.youtube.com/watch?v=1"));
        assert!(trusted("https://discord.gg/abc"));
        assert!(trusted("https://modrinth.com/mod/x"));
        assert!(!trusted("https://google.com@irgendwas-unbekanntes.example/"));
        assert!(!trusted("https://irgendwas-unbekanntes.example/"));
        // Fremde Subdomains und Kurzlinks fragen nach.
        assert!(!trusted("https://jemand.github.io/"));
        assert!(!trusted("https://bit.ly/abc"));
        assert!(!trusted("https://x.a.ssl.fastly.net/"));
    }
}
