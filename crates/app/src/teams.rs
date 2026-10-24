// Teams-Modul: Verbuendete und Gegner bekommen eigene Hitboxen.
//
// Server markieren Teams auf verschiedene Weise, deshalb gibt es mehrere Erkennungen (s.
// Matchers); verbuendet ist, wer bei einer eingeschalteten passt. Jede verlangt, dass BEIDE
// Seiten das Merkmal ueberhaupt haben -- sonst waeren auf einem Server ohne Teams alle
// weissen Namen oder alle ohne Lederruestung "verbuendet".

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Friendly,
    Enemy,
}

impl Side {
    pub const ALL: [Side; 2] = [Side::Friendly, Side::Enemy];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Friendly => "Verbündete",
            Side::Enemy => "Gegner",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Side::Friendly => "teams_friendly",
            Side::Enemy => "teams_enemy",
        }
    }

    pub fn default_style(self) -> TeamHitbox {
        let hue = match self {
            Side::Friendly => 120,
            Side::Enemy => 0,
        };
        TeamHitbox { on: false, nametag: true, hue, saturation: 100, value: 100, opacity: 100, density_px: 2, dotted: false }
    }
}

// Wie die Hitbox einer Seite aussieht, wenn sie ueberschrieben wird. `nametag`: Farbe des
// Namensschilds statt der festen Farbe (hue/saturation/value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamHitbox {
    pub on: bool,
    pub nametag: bool,
    pub hue: u32,
    pub saturation: u32,
    pub value: u32,
    pub opacity: u32,
    pub density_px: u32,
    pub dotted: bool,
}

impl TeamHitbox {
    pub fn encode(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{}",
            self.on, self.nametag, self.hue, self.saturation, self.value, self.opacity, self.density_px, self.dotted
        )
    }

    // Unlesbares laesst die Vorgabe stehen.
    pub fn decode(text: &str, default: TeamHitbox) -> TeamHitbox {
        let teile: Vec<&str> = text.split(',').map(str::trim).collect();
        let [on, nametag, h, s, v, a, d, dotted] = teile[..] else { return default };
        let zahl = |t: &str, min: u32, max: u32| t.parse::<u32>().ok().map(|n| n.clamp(min, max));
        match (zahl(h, 0, 359), zahl(s, 0, 100), zahl(v, 0, 100), zahl(a, 0, u32::MAX), zahl(d, 0, u32::MAX)) {
            (Some(hue), Some(saturation), Some(value), Some(opacity), Some(density_px)) => TeamHitbox {
                on: on == "true",
                nametag: nametag == "true",
                hue,
                saturation,
                value,
                opacity,
                density_px,
                dotted: dotted == "true",
            },
            _ => default,
        }
    }
}

// Welche Erkennungen eingeschaltet sind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matchers {
    // Dasselbe Scoreboard-Team (Vanillas Team-API).
    pub scoreboard: bool,
    // Gleiche Farbe des Namens (Teamfarbe, Tab-Liste oder Namensschild ueber dem Kopf).
    pub name_color: bool,
    // Gleiches erstes Wort vor dem Namen ("[ROT] Name").
    pub prefix: bool,
    // Gleich gefaerbte Lederruestung in einem Slot, den beide gefaerbt tragen.
    pub armor: bool,
}

impl Default for Matchers {
    fn default() -> Self {
        Matchers { scoreboard: true, name_color: true, prefix: false, armor: true }
    }
}

impl Matchers {
    pub const LABELS: [&'static str; 4] = ["Scoreboard-Team", "Namensfarbe", "Prefix", "Rüstungsfarbe"];

    pub fn get(&self, i: usize) -> bool {
        [self.scoreboard, self.name_color, self.prefix, self.armor][i]
    }

    pub fn toggle(&mut self, i: usize) {
        match i {
            0 => self.scoreboard = !self.scoreboard,
            1 => self.name_color = !self.name_color,
            2 => self.prefix = !self.prefix,
            _ => self.armor = !self.armor,
        }
    }

    pub fn encode(&self) -> String {
        format!("{},{},{},{}", self.scoreboard, self.name_color, self.prefix, self.armor)
    }

    pub fn decode(text: &str) -> Matchers {
        let teile: Vec<bool> = text.split(',').map(|t| t.trim() == "true").collect();
        match teile[..] {
            [scoreboard, name_color, prefix, armor] => Matchers { scoreboard, name_color, prefix, armor },
            _ => Matchers::default(),
        }
    }
}

// Was ueber einen Spieler bekannt ist, um sein Team zu bestimmen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub team: Option<String>,
    // Namensfarbe 0xRRGGBB.
    pub color: Option<u32>,
    // Das erste Wort vor dem Namen im angezeigten Namen, ohne Farbcodes.
    pub prefix: Option<String>,
    // Farbe gefaerbter Lederruestung je Slot (Fuesse, Beine, Brust, Kopf).
    pub armor: [Option<u32>; 4],
}

impl Facts {
    // Die Farbe, in der "Namensschild" zeichnet: der Name, sonst die Ruestung (Helm zuerst).
    pub fn display_color(&self) -> Option<u32> {
        self.color.or_else(|| self.armor.iter().rev().find_map(|c| *c))
    }
}

// Das erste Wort vor dem Namen in `text` -- None, wenn der Name vorne steht oder fehlt.
pub fn prefix_before(text: &str, name: &str) -> Option<String> {
    let vor = text[..text.find(name)?].trim();
    vor.split_whitespace().next().map(str::to_string)
}

// Auf welcher Seite steht `other` aus Sicht von `own`?
pub fn side_of(m: &Matchers, own: &Facts, other: &Facts) -> Side {
    fn gleich<T: PartialEq>(a: &Option<T>, b: &Option<T>) -> bool {
        a.is_some() && a == b
    }
    let verbuendet = (m.scoreboard && gleich(&own.team, &other.team))
        || (m.name_color && gleich(&own.color, &other.color))
        || (m.prefix && gleich(&own.prefix, &other.prefix))
        || (m.armor && (0..4).any(|i| gleich(&own.armor[i], &other.armor[i])));
    if verbuendet { Side::Friendly } else { Side::Enemy }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn farbe(c: Option<u32>) -> Facts {
        Facts { color: c, ..Facts::default() }
    }

    #[test]
    fn gleiche_farbe_ist_verbuendet() {
        let m = Matchers::default();
        assert_eq!(side_of(&m, &farbe(Some(0xFF5555)), &farbe(Some(0xFF5555))), Side::Friendly);
        assert_eq!(side_of(&m, &farbe(Some(0xFF5555)), &farbe(Some(0x5555FF))), Side::Enemy);
        assert_eq!(side_of(&m, &farbe(Some(0xFF5555)), &farbe(None)), Side::Enemy);
    }

    #[test]
    fn ohne_merkmal_keine_verbuendeten() {
        let m = Matchers { prefix: true, ..Matchers::default() };
        assert_eq!(side_of(&m, &Facts::default(), &Facts::default()), Side::Enemy);
    }

    #[test]
    fn ruestung_im_selben_slot() {
        let m = Matchers::default();
        let helm = |c| Facts { armor: [None, None, None, Some(c)], ..Facts::default() };
        assert_eq!(side_of(&m, &helm(0x3355FF), &helm(0x3355FF)), Side::Friendly);
        assert_eq!(side_of(&m, &helm(0x3355FF), &helm(0xFF3333)), Side::Enemy);
        let aus = Matchers { armor: false, ..m };
        assert_eq!(side_of(&aus, &helm(0x3355FF), &helm(0x3355FF)), Side::Enemy);
    }

    #[test]
    fn prefix_ist_das_wort_vor_dem_namen() {
        assert_eq!(prefix_before("[ROT] Steve", "Steve").as_deref(), Some("[ROT]"));
        assert_eq!(prefix_before("Steve 19 ❤", "Steve"), None);
        assert_eq!(prefix_before("Alex", "Steve"), None);
    }

    #[test]
    fn stil_ueberlebt_speichern() {
        let st = TeamHitbox { on: true, nametag: false, hue: 200, saturation: 40, value: 90, opacity: 55, density_px: 4, dotted: true };
        assert_eq!(TeamHitbox::decode(&st.encode(), Side::Enemy.default_style()), st);
        assert_eq!(TeamHitbox::decode("kaputt", Side::Enemy.default_style()), Side::Enemy.default_style());
    }
}
