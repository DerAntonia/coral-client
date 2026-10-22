// Session-Info (Modul, s. ModuleId::SessionInfo): Spielzeit seit dem Start des Clients, dazu
// Kills, Tode und Siege. Gezaehlt wird immer, auch mit ausgeschaltetem Modul -- schaltet man
// es mitten in der Runde ein, stimmen die Zahlen trotzdem.
//
// Woher die Zahlen kommen:
// - Kill: die Todesmeldung im Chat nennt einen als Taeter (Schluessel "death.*", s.
//   nbt::death_message) -- so auch Pfeile, Stossen in die Lava usw. Schicken Server eigenen
//   Wortlaut, bleibt das Entity-Event 3 (ServerPlayer.die, an alle Zuschauer): ein Spieler
//   stirbt, den man kurz vorher getroffen hat -- direkt geschlagen oder mit einem
//   zerschlagenen Kristall, einem Anker oder Bett in seiner Naehe (LivingEntity.getKillCredit:
//   100 Ticks). Derselbe Tod ueber beide Wege zaehlt einmal (s. KILL_DEDUPE).
// - Tod: das eigene Leben faellt auf 0, das eigene Entity-Event 3 kommt, die Todesmeldung
//   nennt einen als Opfer -- oder Minispiele setzen einen statt zu toeten in den
//   Zuschauermodus: das zaehlt, wenn man kurz vorher getroffen wurde (SPECTATE_WINDOW). Ohne
//   Treffer ist es kein Tod (etwa beim Betreten einer laufenden Runde).
// - Sieg: kein Paket sagt "gewonnen". Gelesen wird der Text auf dem Bildschirm (Titel und
//   Chat), s. win_in.

use std::collections::HashMap;
use std::time::{Duration, Instant};

// So lange nach dem letzten Treffer zaehlt ein Tod als eigener Kill: Vanillas 100 Ticks plus
// etwas Luft fuer die Laufzeit der Pakete.
const ENGAGE_WINDOW: Duration = Duration::from_secs(6);
// Ein Tod kommt oft doppelt (Leben 0 und Entity-Event im selben Tick).
const DEATH_DEDUPE: Duration = Duration::from_secs(3);
// So lange nach dem letzten eigenen Treffer zaehlt der Wechsel in den Zuschauermodus als Tod.
const SPECTATE_WINDOW: Duration = Duration::from_secs(10);
// Meldung und Entity-Event desselben Todes kommen im selben Tick (erst die Meldung).
const KILL_DEDUPE: Duration = Duration::from_secs(3);
// Titel werden oft mehrmals geschickt (Titel und Untertitel, Wiederholungen); zwei Siege
// liegen mehr als 5 s auseinander.
const WIN_DEDUPE: Duration = Duration::from_secs(5);

// Reichweite der Explosionen, deren Opfer als getroffen gelten: Kristall (Staerke 6) bzw.
// Anker und Bett (5) -- Schaden reicht bis zur doppelten Staerke (ServerExplosion).
pub const CRYSTAL_RADIUS: f64 = 12.0;
pub const BLOCK_EXPLOSION_RADIUS: f64 = 10.0;

// Zeilen unter der Spielzeit, je ein Bit in Settings::session_lines.
pub const LINE_KILLS: u8 = 1;
pub const LINE_DEATHS: u8 = 2;
pub const LINE_KD: u8 = 4;
pub const LINE_WINS: u8 = 8;
pub const LINES: [(u8, &str); 4] = [(LINE_KILLS, "Kills"), (LINE_DEATHS, "Tode"), (LINE_KD, "K/D"), (LINE_WINS, "Siege")];

pub struct Session {
    pub start: Instant,
    pub kills: u32,
    pub deaths: u32,
    pub wins: u32,
    // Entity-ID -> zuletzt getroffen.
    engaged: HashMap<i32, Instant>,
    last_death: Option<Instant>,
    last_hurt: Option<Instant>,
    last_win: Option<Instant>,
    // Zuletzt gezaehlte Kills: Name des Opfers (gross) und wann.
    recent_kills: Vec<(String, Instant)>,
}

impl Session {
    pub fn new(now: Instant) -> Self {
        Session { start: now, kills: 0, deaths: 0, wins: 0, engaged: HashMap::new(), last_death: None, last_hurt: None, last_win: None, recent_kills: Vec::new() }
    }

    pub fn engage(&mut self, entity_id: i32, now: Instant) {
        self.engaged.retain(|_, t| now.duration_since(*t) < ENGAGE_WINDOW);
        self.engaged.insert(entity_id, now);
    }

    // Ein anderer Spieler ist gestorben (Entity-Event; `name` aus der Spielerliste). true, wenn
    // das als Kill zaehlt.
    pub fn player_died(&mut self, entity_id: i32, name: Option<&str>, now: Instant) -> bool {
        let getroffen = self.engaged.remove(&entity_id).is_some_and(|t| now.duration_since(t) < ENGAGE_WINDOW);
        getroffen && self.count_kill(name.unwrap_or_default(), now)
    }

    // Vanillas Todesmeldung: Opfer und Taeter als Text, Teampraefixe inklusive. true, wenn das
    // als Kill zaehlt.
    pub fn death_message(&mut self, victim: &str, killer: Option<&str>, own_name: Option<&str>, now: Instant) -> bool {
        let Some(name) = own_name.map(str::to_uppercase).filter(|n| !n.is_empty()) else { return false };
        if words(victim).contains(&name) {
            self.own_died(now);
            return false;
        }
        if !killer.is_some_and(|k| words(k).contains(&name)) {
            return false;
        }
        // Der Name des Opfers ohne Praefix: das Wort, das in der Spielerliste steht, ist
        // nicht bekannt -- das laengste nimmt "[VIP] Steve" das "Steve".
        let opfer = words(victim).into_iter().max_by_key(String::len).unwrap_or_default();
        self.count_kill(&opfer, now)
    }

    fn count_kill(&mut self, victim: &str, now: Instant) -> bool {
        let victim = victim.to_uppercase();
        self.recent_kills.retain(|(_, t)| now.duration_since(*t) < KILL_DEDUPE);
        if !victim.is_empty() && self.recent_kills.iter().any(|(n, _)| *n == victim) {
            return false;
        }
        self.recent_kills.push((victim, now));
        self.kills += 1;
        true
    }

    // Man selbst hat Schaden genommen.
    pub fn hurt(&mut self, now: Instant) {
        self.last_hurt = Some(now);
    }

    // Wechsel aus Ueberleben/Abenteuer in den Zuschauermodus.
    pub fn became_spectator(&mut self, now: Instant) {
        if self.last_hurt.is_some_and(|t| now.duration_since(t) < SPECTATE_WINDOW) {
            self.own_died(now);
        }
    }

    pub fn own_died(&mut self, now: Instant) {
        if self.last_death.is_some_and(|t| now.duration_since(t) < DEATH_DEDUPE) {
            return;
        }
        self.last_death = Some(now);
        self.deaths += 1;
        // Wer tot ist, hat niemanden mehr im Kampf.
        self.engaged.clear();
    }

    // Text von Titel/Untertitel (`addressed`: spricht den Zuschauer an) oder Chat. true, wenn
    // er als Sieg gezaehlt wurde.
    pub fn text(&mut self, text: &str, own_name: Option<&str>, addressed: bool, now: Instant) -> bool {
        if self.last_win.is_some_and(|t| now.duration_since(t) < WIN_DEDUPE) || !win_in(text, own_name, addressed) {
            return false;
        }
        self.last_win = Some(now);
        self.wins += 1;
        true
    }

    pub fn reset(&mut self, now: Instant) {
        *self = Session::new(now);
    }

    pub fn kd_text(&self) -> String {
        if self.deaths == 0 {
            format!("{}", self.kills)
        } else {
            format!("{:.2}", self.kills as f32 / self.deaths as f32)
        }
    }
}

// "1h 7m 15s" -- Stunden immer, wie auf der Vorlage.
pub fn clock_text(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}h {}m {}s", s / 3600, s / 60 % 60, s % 60)
}

// Gross, Umlaute ausgeschrieben, alles ausser Buchstaben und Ziffern als Trenner. Alte
// Farbcodes ("§6") fallen weg, manche Server schicken sie noch im Text.
fn words(text: &str) -> Vec<String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
            continue;
        }
        for c in c.to_uppercase() {
            match c {
                'Ä' => out.push_str("AE"),
                'Ö' => out.push_str("OE"),
                'Ü' => out.push_str("UE"),
                'ẞ' | 'ß' => out.push_str("SS"),
                c if c.is_alphanumeric() || c == '_' => out.push(c),
                _ => out.push(' '),
            }
        }
    }
    out.split_whitespace().map(str::to_string).collect()
}

fn has_phrase(w: &[String], phrase: &str) -> Option<usize> {
    let p: Vec<&str> = phrase.split(' ').collect();
    w.windows(p.len()).position(|win| win.iter().zip(&p).all(|(a, b)| a == b))
}

// Spricht den Zuschauer an -- braucht keinen Namen (nur Titel, im Chat schreibt das jeder).
const SELF_WIN: [&str; 9] = ["VICTORY", "YOU WIN", "YOU WON", "WINNER WINNER", "SIEG", "GEWONNEN", "DU HAST GEWONNEN", "1ST PLACE", "PLATZ 1"];
// Nennt einen Sieger: zaehlt nur mit dem eigenen Namen. Bei den Hauptwoertern darf er
// irgendwo stehen ("Winner: Name"), bei den Verben muss er davor stehen -- "X won against
// Name" ist eine Niederlage.
const WINNER_NOUNS: [&str; 3] = ["WINNER", "GEWINNER", "SIEGER"];
const WINNER_VERBS: [&str; 5] = ["WON", "WINS", "GEWINNT", "GEWANN", "SIEGT"];
// Diese Woerter heissen das Gegenteil, egal was sonst dasteht.
const LOSS: [&str; 8] = ["DEFEAT", "YOU LOST", "YOU DIED", "GAME OVER", "ELIMINATED", "VERLOREN", "NIEDERLAGE", "AUSGESCHIEDEN"];

// Satz fuer Satz: in "Name was killed. Winner: Steve" gehoeren Name und Sieger nicht zusammen.
pub fn win_in(text: &str, own_name: Option<&str>, addressed: bool) -> bool {
    text.split(['.', '!', '?', '|', '\n']).any(|satz| win_in_sentence(satz, own_name, addressed))
}

fn win_in_sentence(text: &str, own_name: Option<&str>, addressed: bool) -> bool {
    let w = words(text);
    if LOSS.iter().any(|p| has_phrase(&w, p).is_some()) {
        return false;
    }
    if addressed && SELF_WIN.iter().any(|p| has_phrase(&w, p).is_some()) {
        return true;
    }
    let Some(name) = own_name.map(|n| n.to_uppercase()).filter(|n| !n.is_empty()) else { return false };
    let Some(name_at) = w.iter().position(|x| *x == name) else { return false };
    WINNER_NOUNS.iter().any(|p| has_phrase(&w, p).is_some()) || WINNER_VERBS.iter().any(|p| has_phrase(&w, p).is_some_and(|i| i > name_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_nur_nach_treffer() {
        let t0 = Instant::now();
        let mut s = Session::new(t0);
        assert!(!s.player_died(7, Some("Steve"), t0));
        s.engage(7, t0);
        assert!(s.player_died(7, Some("Steve"), t0 + Duration::from_secs(2)));
        // Derselbe Tod ein zweites Mal gemeldet: kein zweiter Kill.
        assert!(!s.player_died(7, Some("Steve"), t0 + Duration::from_secs(2)));
        s.engage(8, t0);
        assert!(!s.player_died(8, Some("Alex"), t0 + Duration::from_secs(10)));
        assert_eq!(s.kills, 1);
    }

    #[test]
    fn kill_aus_todesmeldung() {
        let t0 = Instant::now();
        let mut s = Session::new(t0);
        let ich = Some("CoralClient");
        // Pfeil: nie getroffen im Sinne von engage, die Meldung reicht.
        assert!(s.death_message("[VIP] Steve", Some("CoralClient"), ich, t0));
        // Danach dasselbe Sterben als Entity-Event: zaehlt nicht doppelt.
        s.engage(7, t0);
        assert!(!s.player_died(7, Some("Steve"), t0));
        // Fremder Kill und Tod ohne Taeter: nichts.
        assert!(!s.death_message("Alex", Some("Steve"), ich, t0));
        assert!(!s.death_message("Alex", None, ich, t0));
        assert_eq!(s.kills, 1);
        // Selbst das Opfer: ein Tod.
        assert!(!s.death_message("CoralClient", Some("Steve"), ich, t0));
        assert_eq!(s.deaths, 1);
    }

    #[test]
    fn tod_einmal_je_sterben() {
        let t0 = Instant::now();
        let mut s = Session::new(t0);
        s.own_died(t0);
        s.own_died(t0 + Duration::from_millis(50));
        s.own_died(t0 + Duration::from_secs(20));
        assert_eq!(s.deaths, 2);
        assert_eq!(s.kd_text(), "0.00");
    }

    #[test]
    fn zuschauer_nach_treffer_ist_tod() {
        let t0 = Instant::now();
        let mut s = Session::new(t0);
        // Ohne Treffer (Runde laeuft schon): kein Tod.
        s.became_spectator(t0);
        s.hurt(t0);
        s.became_spectator(t0 + Duration::from_secs(1));
        // Zu lange her.
        s.hurt(t0 + Duration::from_secs(10));
        s.became_spectator(t0 + Duration::from_secs(25));
        assert_eq!(s.deaths, 1);
    }

    #[test]
    fn siege_erkennen() {
        assert!(win_in("§6§lVICTORY!", None, true));
        assert!(!win_in("VICTORY!", None, false));
        assert!(win_in("Du hast gewonnen!", None, true));
        assert!(win_in("Winner: Antonia", Some("antonia"), false));
        assert!(win_in("Antonia won the duel!", Some("Antonia"), false));
        assert!(!win_in("Steve won against Antonia", Some("Antonia"), false));
        assert!(!win_in("Steve won the game", Some("Antonia"), false));
        assert!(!win_in("GAME OVER - Winner: Steve", Some("Antonia"), true));
        assert!(!win_in("Antonia was killed. Winner: Steve", Some("Antonia"), false));
        let t0 = Instant::now();
        let mut s = Session::new(t0);
        assert!(s.text("VICTORY", None, true, t0));
        assert!(!s.text("VICTORY", None, true, t0 + Duration::from_secs(1)));
        assert!(s.text("VICTORY", None, true, t0 + Duration::from_secs(6)));
        assert_eq!(s.wins, 2);
    }

    #[test]
    fn uhr() {
        assert_eq!(clock_text(Duration::from_secs(7 * 60 + 15)), "0h 7m 15s");
        assert_eq!(clock_text(Duration::from_secs(3 * 3600 + 5)), "3h 0m 5s");
    }
}
