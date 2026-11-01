// Hitbox-Anzeige: Arten mit eigenem Schalter, eigener Farbe und eigener Deckkraft.
//
// Zwei Gruppen. Die Entities, die im PvP zaehlen (Spieler, Geschosse, Items am Boden), und
// zwei Bloecke, deren Trefferflaeche man im Kampf kennen will: Spinnennetz (der ganze Block,
// obwohl das Modell nur ein flaches Kreuz ist) und Wasserquellen in der Form, die ein Eimer
// trifft. Alles, was hier nicht steht (Mobs, Ruestungsstaender, Boote), bekommt weiter die
// Farbe der geglaetteten Box und die Deckkraft des Moduls.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitboxKind {
    Player,
    EnderPearl,
    Arrow,
    Trident,
    WindCharge,
    Item,
    Potion,
    Snowball,
    FishingBobber,
    Firework,
    EndCrystal,
    Tnt,
    Cobweb,
    Water,
}

impl HitboxKind {
    pub const COUNT: usize = 14;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Player,
        Self::EnderPearl,
        Self::Arrow,
        Self::Trident,
        Self::WindCharge,
        Self::Item,
        Self::Potion,
        Self::Snowball,
        Self::FishingBobber,
        Self::Firework,
        Self::EndCrystal,
        Self::Tnt,
        Self::Cobweb,
        Self::Water,
    ];

    // Die Entity-Arten, in der Reihenfolge der Liste "Individuelle Farben".
    pub fn entities() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(|k| !k.is_block())
    }

    pub const fn is_block(self) -> bool {
        matches!(self, Self::Cobweb | Self::Water)
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    // Art eines Entities aus seinem Registry-Namen; None = keine eigene Art.
    pub fn of_entity(name: &str) -> Option<Self> {
        Some(match name {
            "player" => Self::Player,
            "ender_pearl" => Self::EnderPearl,
            "arrow" | "spectral_arrow" => Self::Arrow,
            "trident" => Self::Trident,
            "wind_charge" | "breeze_wind_charge" => Self::WindCharge,
            "item" => Self::Item,
            "splash_potion" | "lingering_potion" => Self::Potion,
            "snowball" | "egg" => Self::Snowball,
            "fishing_bobber" => Self::FishingBobber,
            "firework_rocket" => Self::Firework,
            "end_crystal" => Self::EndCrystal,
            "tnt" | "tnt_minecart" => Self::Tnt,
            _ => return None,
        })
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Player => "Spieler",
            Self::EnderPearl => "Enderperle",
            Self::Arrow => "Pfeil",
            Self::Trident => "Dreizack",
            Self::WindCharge => "Windkugel",
            Self::Item => "Item am Boden",
            Self::Potion => "Wurftrank",
            Self::Snowball => "Schneeball, Ei",
            Self::FishingBobber => "Angelhaken",
            Self::Firework => "Feuerwerksrakete",
            Self::EndCrystal => "Endkristall",
            Self::Tnt => "TNT",
            Self::Cobweb => "Spinnennetz",
            Self::Water => "Wasserquelle",
        }
    }

    // Schluessel in der Einstellungsdatei (hitbox_<key>=...).
    pub const fn key(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::EnderPearl => "ender_pearl",
            Self::Arrow => "arrow",
            Self::Trident => "trident",
            Self::WindCharge => "wind_charge",
            Self::Item => "item",
            Self::Potion => "potion",
            Self::Snowball => "snowball",
            Self::FishingBobber => "fishing_bobber",
            Self::Firework => "firework",
            Self::EndCrystal => "end_crystal",
            Self::Tnt => "tnt",
            Self::Cobweb => "cobweb",
            Self::Water => "water",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    // Bekommt diese Art ohne Ueberschreiben eine Box? Items nach dem Schalter oben im Modul,
    // alles andere wie vor den eigenen Arten (s. default_style).
    pub const fn shown_by_default(self, items: bool) -> bool {
        match self {
            Self::Item => items,
            _ => self.default_style().on,
        }
    }

    // Vorgabe: an ist, was die Anzeige schon vor den eigenen Arten gezeigt hat (alles
    // Angreifbare). Pfeile, Dreizacke und Items waren aus und bleiben es, ebenso die Bloecke.
    // Spieler in der Farbe der geglaetteten Box, damit sich beim Umstieg nichts aendert.
    pub const fn default_style(self) -> HitboxStyle {
        let (on, hue, saturation, value) = match self {
            Self::Player => (true, 100, 70, 90),
            Self::EnderPearl => (true, 280, 70, 95),
            Self::Arrow => (false, 0, 0, 100),
            Self::Trident => (false, 180, 60, 90),
            Self::WindCharge => (true, 200, 45, 100),
            Self::Item => (false, 50, 85, 100),
            Self::Potion => (true, 320, 60, 100),
            Self::Snowball => (true, 0, 0, 90),
            Self::FishingBobber => (true, 0, 0, 70),
            Self::Firework => (true, 30, 90, 100),
            Self::EndCrystal => (true, 300, 80, 100),
            Self::Tnt => (true, 0, 90, 100),
            Self::Cobweb => (false, 0, 0, 100),
            Self::Water => (false, 210, 80, 100),
        };
        HitboxStyle { on, custom: false, hue, saturation, value, alpha: 100 }
    }
}

// Ueberschreiben, Sichtbar, Farbe (Ton in Grad, Saettigung/Helligkeit in Prozent) und
// Deckkraft in Prozent. Bei den Entities gelten Sichtbar, Farbe und Deckkraft nur mit
// Ueberschreiben, sonst die Vorgaben (s. HitboxKind::shown_by_default, Farbe von "Geglättet").
// Die Bloecke haben keine geglaettete Box: fuer sie ist Sichtbar der Schalter oben im Modul
// und die Farbe immer ihre eigene. Die Deckkraft wirkt zusaetzlich zur Deckkraft des Moduls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HitboxStyle {
    pub on: bool,
    pub custom: bool,
    pub hue: u32,
    pub saturation: u32,
    pub value: u32,
    pub alpha: u32,
}

impl HitboxStyle {
    pub fn encode(&self) -> String {
        format!("{},{},{},{},{},{}", self.on, self.hue, self.saturation, self.value, self.alpha, self.custom)
    }

    // Unlesbares laesst die Vorgabe stehen. Ohne sechstes Feld (aeltere Dateien) ist
    // Ueberschreiben aus.
    pub fn decode(text: &str, default: Self) -> Self {
        let teile: Vec<&str> = text.split(',').collect();
        let (on, h, s, v, a, custom) = match teile[..] {
            [on, h, s, v, a] => (on, h, s, v, a, "false"),
            [on, h, s, v, a, custom] => (on, h, s, v, a, custom),
            _ => return default,
        };
        let zahl = |t: &str, max: u32| t.trim().parse::<u32>().ok().map(|n| n.min(max));
        match (zahl(h, 359), zahl(s, 100), zahl(v, 100), zahl(a, 100)) {
            (Some(hue), Some(saturation), Some(value), Some(alpha)) => {
                Self { on: on.trim() == "true", custom: custom.trim() == "true", hue, saturation, value, alpha }
            }
            _ => default,
        }
    }
}

// Kann ein leerer Eimer hier Wasser aufnehmen? Nur aus einer Quelle: einem Wasserblock auf
// Stufe 0, einem Block mit Wasser darin (waterlogged) oder einer Blasensaeule. Seegras und
// Seetang stehen zwar ebenfalls in einer Quelle, lassen sich aber nicht ausschoepfen.
// Getroffen wird die Fluessigkeit als volles Quadrat bis zu ihrer Hoehe (s.
// world::fluid_height_at), nicht die abgeschraegte Oberflaeche, die man sieht.
pub fn bucket_source(state: u32) -> bool {
    use world::state_props::property;
    match world::block_name(state) {
        Some("water") => property(state, "level") == Some("0"),
        Some("bubble_column") => true,
        Some(_) => property(state, "waterlogged") == Some("true"),
        None => false,
    }
}

pub fn default_styles() -> [HitboxStyle; HitboxKind::COUNT] {
    HitboxKind::ALL.map(HitboxKind::default_style)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reihenfolge_passt_zum_index() {
        for (i, k) in HitboxKind::ALL.into_iter().enumerate() {
            assert_eq!(k.index(), i);
            assert_eq!(HitboxKind::from_key(k.key()), Some(k));
        }
    }

    #[test]
    fn alle_entity_namen_gibt_es_in_der_registry() {
        for name in [
            "player",
            "ender_pearl",
            "arrow",
            "spectral_arrow",
            "trident",
            "wind_charge",
            "breeze_wind_charge",
            "item",
            "splash_potion",
            "lingering_potion",
            "snowball",
            "egg",
            "fishing_bobber",
            "firework_rocket",
            "end_crystal",
            "tnt",
            "tnt_minecart",
        ] {
            assert!(world::entity_types::ENTITY_TYPE_NAMES.contains(&name), "{name} fehlt in der Registry");
            assert!(HitboxKind::of_entity(name).is_some());
        }
    }

    #[test]
    fn eimer_nimmt_nur_quellen() {
        let state = |name: &str, props: &[(&str, &str)]| {
            (0..world::state_count() as u32)
                .find(|&s| world::block_name(s) == Some(name) && props.iter().all(|(k, v)| world::state_props::property(s, k) == Some(*v)))
                .unwrap_or_else(|| panic!("{name} {props:?} unbekannt"))
        };
        assert!(bucket_source(state("water", &[("level", "0")])));
        assert!(!bucket_source(state("water", &[("level", "3")])));
        assert!(bucket_source(state("oak_stairs", &[("waterlogged", "true")])));
        assert!(!bucket_source(state("oak_stairs", &[("waterlogged", "false")])));
        assert!(bucket_source(state("bubble_column", &[])));
        assert!(!bucket_source(state("seagrass", &[])));
        assert!(!bucket_source(state("cobweb", &[])));
    }

    #[test]
    fn stil_rundlauf_und_kaputte_zeilen() {
        let s = HitboxStyle { on: true, custom: true, hue: 123, saturation: 45, value: 67, alpha: 89 };
        assert_eq!(HitboxStyle::decode(&s.encode(), HitboxKind::Item.default_style()), s);
        let alt = HitboxStyle::decode("true,1,2,3,4", HitboxKind::Item.default_style());
        assert_eq!((alt.on, alt.custom, alt.alpha), (true, false, 4), "alte Zeile ohne Ueberschreiben");
        let d = HitboxKind::Water.default_style();
        assert_eq!(HitboxStyle::decode("true,1,2", d), d);
        assert_eq!(HitboxStyle::decode("true,x,2,3,4", d), d);
    }
}
