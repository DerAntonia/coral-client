// Item ansehen (wie "Inspect" in Counter-Strike): auf Tastendruck dreht die Ego-Hand das
// gehaltene Item einmal vor der Kamera, je nach Art anders. Eigene Animationen, keine Vorlage
// aus Vanilla.
//
// Alles spielt im Sichtraum (Kamera im Ursprung, Blick entlang -Z, +X rechts, +Y oben) und
// dreht um einen festen Punkt: beim Item das Handgelenk (der Ursprung von
// first_person_item_transform), bei leerer Hand das untere Ende des Arms. Gerechnet wird fuer
// die rechte Hand, die linke spiegelt (X verschoben, Drehungen um Y und Z umgekehrt).

use crate::entity_model::{rotation_x_deg, rotation_z_deg, Affine};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sword,
    Axe,
    Tool,
    Mace,
    Spear,
    Bow,
    Crossbow,
    Shield,
    Watch,
    Totem,
    FishingRod,
    Igniter,
    Block,
    Consumable,
    Other,
    EmptyHand,
    // Nah an die Augen gehalten (leichte Items, oder die andere Hand ist belegt).
    CloseLook,
    // Traenke und Wurfitems (Schneeball, Ei, Perle, ...): zweimal hochgeworfen wie Werkzeuge.
    Throwable,
    // Jonglieren (s. juggle): laeuft bis zur naechsten Aktion, ohne eigene Schluesselbilder.
    Juggle(crate::juggle::Trick),
}

// Welche Animation zu einem Item gehoert. None: das Item behaelt seine eigene Ansicht (Karte,
// Fernrohr). `block`: das Item ist ein Block, `consumable`: es wird gegessen oder getrunken.
// Flache Items ohne Griff (Totem, Essen, Traenke) drehen sich beim Ansehen um ihre eigene Mitte,
// nicht um den Griff eines Werkzeugs.
impl Kind {
    #[must_use] 
    pub const fn turns_around_center(self) -> bool {
        matches!(self, Self::Totem | Self::Consumable | Self::CloseLook)
    }

    // Ueberschlagen sich im Flug (s. tool_toss_transform): in der Hand um den Griff gekippt, in
    // der Luft um die eigene Mitte gedreht.
    #[must_use] 
    pub const fn flips(self) -> bool {
        matches!(self, Self::Axe | Self::Tool | Self::Throwable)
    }

    // Hochgeworfen: das Item fliegt allein, die Hand bleibt unten und faengt es.
    #[must_use] 
    pub const fn tossed(self) -> bool {
        matches!(self, Self::Mace | Self::Axe | Self::Tool | Self::Throwable)
    }
}

#[must_use] 
pub fn kind(name: &str, block: bool, consumable: bool) -> Option<Kind> {
    Some(match name {
        "filled_map" | "map" | "spyglass" => return None,
        "mace" => Kind::Mace,
        "trident" => Kind::Spear,
        "bow" => Kind::Bow,
        "crossbow" => Kind::Crossbow,
        "shield" => Kind::Shield,
        "compass" | "recovery_compass" | "clock" => Kind::Watch,
        "totem_of_undying" => Kind::Totem,
        "fishing_rod" => Kind::FishingRod,
        "flint_and_steel" | "fire_charge" => Kind::Igniter,
        _ if name.ends_with("_sword") => Kind::Sword,
        _ if name.ends_with("_spear") => Kind::Spear,
        _ if name.ends_with("_axe") => Kind::Axe,
        _ if name.ends_with("_pickaxe") || name.ends_with("_shovel") || name.ends_with("_hoe") => Kind::Tool,
        "potion" | "splash_potion" | "lingering_potion" | "snowball" | "egg" | "blue_egg" | "brown_egg" | "ender_pearl" | "ender_eye" | "experience_bottle" | "wind_charge" => Kind::Throwable,
        _ if consumable => Kind::Consumable,
        _ if block => Kind::Block,
        _ => Kind::Other,
    })
}

#[derive(Clone, Copy)]
enum Ease {
    // Weich an- und auslaufend.
    Smooth,
    // Schnell los, langsam an (Wurf nach oben).
    Out,
    // Langsam los, schnell an (Fall).
    In,
}

// Ein Schluesselbild: Zeit 0..1, Verschiebung, Drehung um die Sichtachsen (X, Y, Z in Grad,
// angewandt Y, dann X, dann Z) und Drehung um die Laengsachse des Items. `ease` gilt fuer den
// Weg vom vorigen Bild hierher.
#[derive(Clone, Copy)]
struct Key {
    t: f32,
    pos: [f32; 3],
    rot: [f32; 3],
    spin: f32,
    ease: Ease,
    // Die Drehung gleichmaessig statt mit `ease` (ein Item dreht sich im Flug gleich schnell).
    rot_linear: bool,
}

const fn k(t: f32, pos: [f32; 3], rot: [f32; 3]) -> Key {
    Key { t, pos, rot, spin: 0.0, ease: Ease::Smooth, rot_linear: false }
}

const fn ks(t: f32, pos: [f32; 3], rot: [f32; 3], spin: f32) -> Key {
    Key { t, pos, rot, spin, ease: Ease::Smooth, rot_linear: false }
}

const fn ke(t: f32, pos: [f32; 3], rot: [f32; 3], ease: Ease) -> Key {
    Key { t, pos, rot, spin: 0.0, ease, rot_linear: false }
}

// Wie ke, die Drehung aber gleichmaessig (im Flug).
const fn kf(t: f32, pos: [f32; 3], rot: [f32; 3], ease: Ease) -> Key {
    Key { t, pos, rot, spin: 0.0, ease, rot_linear: true }
}

const NULL: [f32; 3] = [0.0; 3];

// Das Handgelenk der rechten Hand im Sichtraum (s. first_person_item_transform).
const ITEM_PIVOT: [f32; 3] = [0.56, -0.52, -0.72];
// Laengsachse eines Handheld-Items (Griff -> Spitze) im Sichtraum der rechten Hand.
const ITEM_AXIS: [f32; 3] = [0.0, 0.938, 0.346];
// Leere Hand: unteres Ende des Arms und die Richtung zur Faust (s. first_person_arm_transform).
const ARM_PIVOT: [f32; 3] = [0.55, -1.0, -0.6];
const ARM_AXIS: [f32; 3] = [0.159, 0.851, -0.501];

const SWORD: &[Key] = &[
    k(0.0, NULL, NULL),
    // Klinge zur Kamera.
    // Fast bis zur Bildmitte (die Hand steht bei x 0.56).
    k(0.16, [-0.38, 0.10, 0.05], [-10.0, -32.0, 10.0]),
    k(0.34, [-0.42, 0.11, 0.055], [-12.0, -38.0, 12.0]),
    // Flache Seite.
    k(0.52, [-0.45, 0.12, 0.06], [-5.0, 85.0, 20.0]),
    k(0.70, [-0.45, 0.12, 0.06], [-5.0, 95.0, 24.0]),
    // Kurzer Dreh im Handgelenk zurueck.
    ke(0.82, [-0.18, 0.05, 0.025], [0.0, -25.0, -15.0], Ease::In),
    k(1.0, NULL, NULL),
];

// Werkzeuge (Axt, Spitzhacke, Schaufel, Hacke): zweimal hochgeworfen, dabei einmal um die
// waagerechte Achse ueberschlagen, um die eigene Mitte (Kind::turns_around_center). Hoch Ease::Out,
// runter Ease::In: das ist genau eine Wurfparabel. Die Hand bleibt unten (s. Kind::tossed).
// Alles um die Handlage (hold::TOOL_TOSS_HAND) nach rechts oben versetzt.
const TX: f32 = crate::hold::TOOL_TOSS_HAND[0];
const TY: f32 = crate::hold::TOOL_TOSS_HAND[1];
// Vor jedem Wurf erst nach vorn gelehnt (TOOL_LEAN, gegen die Flugdrehung), dann
// hochgeschwungen.
const TOOL_LEAN: f32 = -60.0;
const TOOL_TOSS: &[Key] = &[
    k(0.0, NULL, NULL),
    k(0.10, [TX, TY - 0.03, 0.0], [TOOL_LEAN, 0.0, 0.0]),
    k(0.14, [TX, TY - 0.06, 0.0], [TOOL_LEAN, 0.0, 0.0]),
    // Im Flug gleich schnell gedreht: oben genau die Haelfte des Wegs.
    kf(0.26, [TX, TY + 0.55, 0.0], [f32::midpoint(TOOL_LEAN, 360.0), 0.0, 0.0], Ease::Out),
    kf(0.38, [TX, TY - 0.06, 0.0], [360.0, 0.0, 0.0], Ease::In),
    k(0.44, [TX, TY + 0.015, 0.0], [360.0, 0.0, 0.0]),
    k(0.52, [TX, TY - 0.03, 0.0], [360.0 + TOOL_LEAN, 0.0, 0.0]),
    k(0.56, [TX, TY - 0.06, 0.0], [360.0 + TOOL_LEAN, 0.0, 0.0]),
    kf(0.68, [TX, TY + 0.55, 0.0], [360.0 + f32::midpoint(TOOL_LEAN, 360.0), 0.0, 0.0], Ease::Out),
    kf(0.80, [TX, TY - 0.06, 0.0], [720.0, 0.0, 0.0], Ease::In),
    k(0.87, [TX, TY + 0.015, 0.0], [720.0, 0.0, 0.0]),
    k(1.0, NULL, [720.0, 0.0, 0.0]),
];

// Traenke und Wurfitems: dieselben zwei Wuerfe wie TOOL_TOSS, aber ohne Vorlehnen und ohne
// Ueberschlag -- sie fliegen gerade hoch und wieder in die Hand.
const THROWABLE_TOSS: &[Key] = &[
    k(0.0, NULL, NULL),
    k(0.14, [TX, TY - 0.06, 0.0], NULL),
    ke(0.26, [TX, TY + 0.55, 0.0], NULL, Ease::Out),
    ke(0.38, [TX, TY - 0.06, 0.0], NULL, Ease::In),
    k(0.44, [TX, TY + 0.015, 0.0], NULL),
    k(0.56, [TX, TY - 0.06, 0.0], NULL),
    ke(0.68, [TX, TY + 0.55, 0.0], NULL, Ease::Out),
    ke(0.80, [TX, TY - 0.06, 0.0], NULL, Ease::In),
    k(0.87, [TX, TY + 0.015, 0.0], NULL),
    k(1.0, NULL, NULL),
];

const MACE: &[Key] = &[
    k(0.0, NULL, NULL),
    // Zweimal in der Hand gewogen.
    k(0.08, [0.0, -0.05, 0.0], [-5.0, 0.0, 0.0]),
    k(0.16, [0.0, 0.03, 0.0], NULL),
    k(0.24, [0.0, -0.05, 0.0], [-5.0, 0.0, 0.0]),
    k(0.32, [0.0, 0.0, 0.0], NULL),
    // Ausholen, hochwerfen, fangen.
    k(0.37, [0.0, -0.07, 0.0], [5.0, 0.0, 0.0]),
    ke(0.56, [-0.05, 0.40, 0.0], [-180.0, 0.0, 0.0], Ease::Out),
    ke(0.74, [0.0, -0.08, 0.0], [-360.0, 0.0, 0.0], Ease::In),
    k(0.85, [0.0, 0.02, 0.0], [-360.0, 0.0, 0.0]),
    k(1.0, NULL, [-360.0, 0.0, 0.0]),
];

const SPEAR: &[Key] = &[
    k(0.0, NULL, NULL),
    k(0.15, [-0.12, 0.08, 0.04], NULL),
    // Einmal um die Laengsachse gewirbelt.
    ks(0.50, [-0.12, 0.08, 0.04], NULL, 360.0),
    // Spitze nach vorne.
    ks(0.70, [-0.08, 0.06, 0.0], [-70.0, 0.0, 0.0], 360.0),
    ks(0.85, [-0.08, 0.06, 0.0], [-72.0, 0.0, 0.0], 360.0),
    ks(1.0, NULL, NULL, 360.0),
];

const BOW: &[Key] = &[
    k(0.0, NULL, NULL),
    // Seitlich gedreht.
    k(0.18, [-0.15, 0.08, 0.04], [0.0, 0.0, 60.0]),
    // Blick entlang der Sehne, dann das Zupfen (s. extra).
    k(0.40, [-0.17, 0.10, 0.05], [10.0, 55.0, 60.0]),
    k(0.78, [-0.17, 0.10, 0.05], [12.0, 58.0, 62.0]),
    k(1.0, NULL, NULL),
];

const CROSSBOW: &[Key] = &[
    k(0.0, NULL, NULL),
    // Zur Seite gekippt wie eine Pistole.
    k(0.20, [-0.15, 0.06, 0.04], [0.0, 0.0, 75.0]),
    // Blick auf den Mechanismus.
    k(0.45, [-0.17, 0.08, 0.05], [15.0, 35.0, 80.0]),
    k(0.70, [-0.17, 0.08, 0.05], [18.0, 40.0, 82.0]),
    k(0.88, [-0.05, 0.02, 0.01], [0.0, 0.0, -8.0]),
    k(1.0, NULL, NULL),
];

// Fuer die rechte Hand gerechnet; in der Nebenhand (links) gespiegelt.
const SHIELD: &[Key] = &[
    k(0.0, NULL, NULL),
    // Nach vorne gedreht, das Muster zur Kamera.
    k(0.30, [-0.25, 0.10, 0.025], [0.0, -150.0, 0.0]),
    k(0.70, [-0.25, 0.10, 0.025], [5.0, -160.0, 3.0]),
    k(1.0, NULL, NULL),
];

const WATCH: &[Key] = &[
    k(0.0, NULL, NULL),
    // Nah ans Gesicht, gekippt wie eine Armbanduhr.
    k(0.25, [-0.28, 0.20, 0.14], [20.0, 70.0, -10.0]),
    k(0.75, [-0.28, 0.21, 0.14], [25.0, 78.0, -6.0]),
    k(1.0, NULL, NULL),
];

const TOTEM: &[Key] = &[
    k(0.0, NULL, NULL),
    // Hochgehalten, Gesicht zur Kamera, leichte Drehung (s. extra).
    k(0.25, [-0.20, 0.25, 0.05], [0.0, 80.0, 0.0]),
    k(0.75, [-0.20, 0.26, 0.05], [0.0, 85.0, 0.0]),
    k(1.0, NULL, NULL),
];

const FISHING_ROD: &[Key] = &[
    k(0.0, NULL, NULL),
    k(0.25, [0.0, 0.02, 0.0], [10.0, 0.0, 0.0]),
    // Angeschnippt.
    ke(0.35, [0.0, 0.0, -0.01], [-30.0, 0.0, 0.0], Ease::In),
    k(0.50, [0.0, 0.01, 0.0], [5.0, 0.0, 0.0]),
    k(0.65, NULL, [-12.0, 0.0, 0.0]),
    k(0.80, NULL, [2.0, 0.0, 0.0]),
    k(1.0, NULL, NULL),
];

const IGNITER: &[Key] = &[
    k(0.0, NULL, NULL),
    k(0.25, [-0.15, 0.08, 0.05], [0.0, 70.0, 0.0]),
    k(0.52, [-0.15, 0.08, 0.05], [0.0, 72.0, 0.0]),
    // Einmal angeklickt.
    ke(0.58, [-0.15, 0.07, 0.05], [0.0, 72.0, -15.0], Ease::In),
    k(0.66, [-0.15, 0.08, 0.05], [0.0, 72.0, 0.0]),
    k(0.80, [-0.15, 0.08, 0.05], [0.0, 70.0, 0.0]),
    k(1.0, NULL, NULL),
];

const BLOCK: &[Key] = &[
    k(0.0, NULL, NULL),
    k(0.15, [-0.15, 0.10, 0.05], NULL),
    // Einmal ganz um die Hochachse.
    ke(0.85, [-0.15, 0.10, 0.05], [0.0, 360.0, 0.0], Ease::Smooth),
    k(1.0, NULL, [0.0, 360.0, 0.0]),
];

const CONSUMABLE: &[Key] = &[
    k(0.0, NULL, NULL),
    // Zur Kamera gedreht, das Schwenken kommt aus extra.
    k(0.25, [-0.18, 0.10, 0.06], [0.0, 70.0, 0.0]),
    k(0.75, [-0.18, 0.10, 0.06], [0.0, 72.0, 0.0]),
    k(1.0, NULL, NULL),
];

const OTHER: &[Key] = &[
    k(0.0, NULL, NULL),
    k(0.15, [-0.15, 0.08, 0.05], NULL),
    // Einmal umgedreht: Vorder- und Rueckseite.
    k(0.50, [-0.15, 0.08, 0.05], [0.0, 180.0, 0.0]),
    k(0.85, [-0.15, 0.08, 0.05], [0.0, 360.0, 0.0]),
    k(1.0, NULL, [0.0, 360.0, 0.0]),
];

const CLOSE_LOOK: &[Key] = &[
    k(0.0, NULL, NULL),
    // Dicht vor die Augen mittig vor die Kamera (pos = Anteil des Wegs dorthin, s.
    // hold::CLOSE_LOOK_TARGET), erst von der einen, dann von der anderen Seite angesehen.
    k(0.22, [1.0, 1.0, 1.0], [8.0, -15.0, 0.0]),
    k(0.50, [1.0, 1.0, 1.0], [12.0, 15.0, 6.0]),
    k(0.78, [1.0, 1.0, 1.0], [6.0, 0.0, 0.0]),
    k(1.0, NULL, NULL),
];

const EMPTY_HAND: &[Key] = &[
    k(0.0, NULL, NULL),
    // Hand zur Bildmitte, Handflaeche zur Kamera (Fingerspiel s. extra).
    ks(0.25, [-0.05, 0.08, 0.025], [0.0, 0.0, 22.0], 60.0),
    ks(0.75, [-0.05, 0.09, 0.025], [0.0, 0.0, 24.0], 65.0),
    k(1.0, NULL, NULL),
];

impl Kind {
    // Dauer in Ticks.
    #[must_use] 
    pub const fn ticks(self) -> f32 {
        match self {
            Self::Sword | Self::Spear | Self::Bow | Self::Crossbow | Self::Block => 48.0,
            Self::Mace => 52.0,
            // Werkzeuge: 0.12 von 44 Ticks steigen sie 0.61 hoch, das ist dieselbe Schwerkraft
            // wie beim Jonglieren (juggle::GRAVITY).
            Self::Watch | Self::Totem | Self::Axe | Self::Tool | Self::Throwable => 44.0,
            Self::Shield | Self::Consumable | Self::Other => 40.0,
            Self::Igniter | Self::EmptyHand => 36.0,
            Self::FishingRod => 32.0,
            Self::CloseLook => 50.0,
            Self::Juggle(_) => f32::INFINITY,
        }
    }

    const fn keys(self) -> &'static [Key] {
        match self {
            Self::Sword => SWORD,
            Self::Axe | Self::Tool => TOOL_TOSS,
            Self::Throwable => THROWABLE_TOSS,
            Self::Mace => MACE,
            Self::Spear => SPEAR,
            Self::Bow => BOW,
            Self::Crossbow => CROSSBOW,
            Self::Shield => SHIELD,
            Self::Watch => WATCH,
            Self::Totem => TOTEM,
            Self::FishingRod => FISHING_ROD,
            Self::Igniter => IGNITER,
            Self::Block => BLOCK,
            Self::Consumable => CONSUMABLE,
            Self::Other => OTHER,
            Self::EmptyHand => EMPTY_HAND,
            Self::CloseLook => CLOSE_LOOK,
            // Ungenutzt: transform und third_person lassen das Jonglieren aus.
            Self::Juggle(_) => OTHER,
        }
    }
}

fn ease(e: Ease, u: f32) -> f32 {
    match e {
        Ease::Smooth => u * u * (3.0 - 2.0 * u),
        Ease::Out => 1.0 - (1.0 - u) * (1.0 - u),
        Ease::In => u * u,
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], f: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
}

// Verschiebung, Drehung und Laengsdrehung zum Zeitpunkt t (0..1).
fn sample(keys: &[Key], t: f32) -> ([f32; 3], [f32; 3], f32) {
    let t = t.clamp(0.0, 1.0);
    let i = keys.iter().rposition(|key| key.t <= t).unwrap_or(0).min(keys.len() - 2);
    let (a, b) = (keys[i], keys[i + 1]);
    let u = ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0);
    let f = ease(b.ease, u);
    let fr = if b.rot_linear { u } else { f };
    (lerp3(a.pos, b.pos, f), lerp3(a.rot, b.rot, fr), a.spin + (b.spin - a.spin) * f)
}

// Huelle 0..1..0 zwischen von und bis, fuer die kleinen Zusatzbewegungen.
fn window(t: f32, von: f32, bis: f32) -> f32 {
    if t <= von || t >= bis {
        return 0.0;
    }
    ((t - von) / (bis - von) * std::f32::consts::PI).sin()
}

// Was die Schluesselbilder nicht koennen: Zupfen, Schwenken, Fingerspiel.
fn extra(kind: Kind, t: f32) -> ([f32; 3], [f32; 3]) {
    let tau = std::f32::consts::TAU;
    match kind {
        // Die Sehne gezupft: schnelles, abklingendes Zittern.
        Kind::Bow => {
            let w = window(t, 0.52, 0.70) * (1.0 - ((t - 0.52) / 0.18).clamp(0.0, 1.0));
            ([0.012 * w * (t * tau * 30.0).sin(), 0.0, 0.0], [0.0, 0.0, 2.5 * w * (t * tau * 30.0).sin()])
        }
        Kind::Consumable => (NULL, [0.0, 0.0, 12.0 * window(t, 0.2, 0.8) * (t * tau * 2.5).sin()]),
        Kind::Totem => (NULL, [0.0, 0.0, 8.0 * window(t, 0.2, 0.8) * (t * tau * 1.5).sin()]),
        Kind::EmptyHand => (NULL, [1.5 * window(t, 0.3, 0.72) * (t * tau * 9.0).sin(), 0.0, 2.0 * window(t, 0.3, 0.72) * (t * tau * 7.0).sin()]),
        _ => (NULL, NULL),
    }
}

// Die Zusatz-Transformation zum Zeitpunkt t (0..1): VOR die gewohnte Kette der Hand setzen
// (Item: first_person_item_transform bzw. die Pose des Items, leere Hand:
// first_person_arm_transform).
#[must_use] 
pub fn transform(kind: Kind, t: f32, right: bool) -> Affine {
    transform_at(kind, t, right, None)
}

// Hochgeworfen (Kind::tossed): wohin die Hand zum Zeitpunkt t gegen die Ruhe verschoben ist.
// Sie macht das Ausholen mit, aber nicht Flug und Drehung: nach oben hoechstens knapp ueber ihre
// Lage (beim Werkzeug die nach rechts oben versetzte, hold::TOOL_TOSS_HAND).
#[must_use] 
pub fn toss_hand_offset(kind: Kind, t: f32, right: bool) -> [f32; 3] {
    let s = if right { 1.0 } else { -1.0 };
    let (pos, _, _) = sample(kind.keys(), t);
    let oben = if kind == Kind::Mace { 0.03 } else { TY + 0.03 };
    [s * pos[0], pos[1].min(oben), pos[2]]
}

// Werkzeug-Wurf (Kind::flips): in der Hand kippt das Werkzeug um den Griff (`griff`, die Spitze
// bewegt sich), in der Luft dreht es sich um seine Mitte (`mitte`), beide im Sichtraum, schon
// gespiegelt. Dazwischen gleitend nach der Hoehe ueber der Hand; beim Fangen ist die Drehung ganz
// herum, da faellt beides zusammen.
#[must_use] 
pub fn tool_toss_transform(kind: Kind, t: f32, right: bool, griff: [f32; 3], mitte: [f32; 3]) -> Affine {
    let um_griff = transform_at(kind, t, right, Some(griff));
    let (pos, _, _) = sample(kind.keys(), t);
    let u = ((pos[1] - TY - 0.02) / 0.12).clamp(0.0, 1.0);
    let in_luft = u * u * (3.0 - 2.0 * u);
    // Um die Mitte statt um den Griff gedreht liegt alles um (1 - R)(mitte - griff) versetzt.
    let d = sub(mitte, griff);
    let rd = sub(um_griff.apply(mitte), um_griff.apply(griff));
    Affine::translation([0, 1, 2].map(|i| in_luft * (d[i] - rd[i]))).then(um_griff)
}

// Wie weit das Nah-Ansehen zum Zeitpunkt t herangeholt ist (0..1).
#[must_use] 
pub fn close_look_weight(t: f32) -> f32 {
    sample(CLOSE_LOOK, t).0[0]
}

// Die Groesse des Items dabei (1 = wie in der Hand, s. hold::CLOSE_LOOK_SCALE).
#[must_use] 
pub fn close_look_size(t: f32) -> f32 {
    1.0 + (crate::hold::CLOSE_LOOK_SCALE - 1.0) * close_look_weight(t)
}

// Nah ansehen (Kind::CloseLook): wie transform_at um `mitte`, dazu das Item so gedreht, dass
// seine flache Seite (Normale `normale` im Sichtraum) zur Kamera zeigt, mit dem Weg dorthin
// eingeblendet.
#[must_use] 
pub fn close_look_transform(t: f32, right: bool, mitte: [f32; 3], normale: [f32; 3]) -> Affine {
    let basis = transform_at(Kind::CloseLook, t, right, Some(mitte));
    let anteil = sample(CLOSE_LOOK, t).0[0];
    let n = norm(normale);
    // Zur Kamera (+Z) oder von ihr weg, je nachdem was naeher liegt: flache Items sehen von
    // beiden Seiten gleich aus.
    let ziel = if n[2] >= 0.0 { [0.0, 0.0, 1.0] } else { [0.0, 0.0, -1.0] };
    let achse = [n[1] * ziel[2] - n[2] * ziel[1], n[2] * ziel[0] - n[0] * ziel[2], n[0] * ziel[1] - n[1] * ziel[0]];
    let sin = (achse[0] * achse[0] + achse[1] * achse[1] + achse[2] * achse[2]).sqrt();
    if sin < 1e-5 {
        let groesse = close_look_size(t);
        return basis.then(Affine::translation(mitte)).then(Affine::scale([groesse; 3])).then(Affine::translation([-mitte[0], -mitte[1], -mitte[2]]));
    }
    let winkel = sin.atan2(n[0] * ziel[0] + n[1] * ziel[1] + n[2] * ziel[2]);
    let groesse = close_look_size(t);
    let zur_kamera = Affine::translation(mitte)
        .then(Affine::scale([groesse; 3]))
        .then(Affine::rotation_axis_angle(anteil * winkel, norm(achse)))
        .then(Affine::translation([-mitte[0], -mitte[1], -mitte[2]]));
    basis.then(zur_kamera)
}

// Wie transform, aber um `mitte` (im Sichtraum, schon gespiegelt) statt um den festen Drehpunkt.
#[must_use] 
pub fn transform_at(kind: Kind, t: f32, right: bool, mitte: Option<[f32; 3]>) -> Affine {
    if matches!(kind, Kind::Juggle(_)) {
        return Affine::IDENTITY;
    }
    let s = if right { 1.0 } else { -1.0 };
    let (pos, rot, spin) = sample(kind.keys(), t);
    let (extra_pos, extra_rot) = extra(kind, t);
    let (pivot, axis) = if kind == Kind::EmptyHand { (ARM_PIVOT, ARM_AXIS) } else { (ITEM_PIVOT, ITEM_AXIS) };
    let p = mitte.unwrap_or([s * pivot[0], pivot[1], pivot[2]]);
    // Nah ansehen: die Mitte des Items zum festen Ziel vor der Kamera, egal wo es gehalten wird.
    let pos = if kind == Kind::CloseLook {
        let z = crate::hold::CLOSE_LOOK_TARGET;
        [s * pos[0] * (s * z[0] - p[0]), pos[1] * (z[1] - p[1]), pos[2] * (z[2] - p[2])]
    } else {
        pos
    };
    Affine::translation([p[0] + s * (pos[0] + extra_pos[0]), p[1] + pos[1] + extra_pos[1], p[2] + pos[2] + extra_pos[2]])
        .then(Affine::rotation_y_deg(s * (rot[1] + extra_rot[1])))
        .then(rotation_x_deg(rot[0] + extra_rot[0]))
        .then(rotation_z_deg(s * (rot[2] + extra_rot[2])))
        .then(Affine::rotation_axis_angle((s * spin).to_radians(), [s * axis[0], axis[1], axis[2]]))
        .then(Affine::translation([-p[0], -p[1], -p[2]]))
}

// Der Arm am gehaltenen Item in der Ego-Ansicht. Vanilla zeichnet dort keinen Arm, sobald ein
// Item in der Hand liegt -- beim schnellen Umsehen oder beim Ansehen schwebt es dann sichtbar.
// Hier haelt die Faust immer das untere Griffende, der Arm laeuft von dort zur Schulter; in
// Ruhe liegt er knapp unter dem Bildrand.
// Die Faust am Ego-Arm in Ruhe (s. first_person_arm_transform): kurz vor dem Ende des
// Arm-Quaders, und die Richtung vom Ende zur Faust.
const FIST: [f32; 3] = [0.714, -0.499, -1.010];
const FOREARM: [f32; 3] = [0.369, 0.541, -0.757];
// Dorthin laeuft der Arm vom Griff aus: nach rechts unten aus dem Bild.
const SHOULDER: [f32; 3] = [0.7, -1.4, -0.3];
// So weit liegt die Faust (FIST) vom Griff zur Schulter hin: das Ende des Arm-Quaders liegt
// 0.11 hinter FIST, das Item steckt also knapp ein Pixel tief oben in der Faust. Mehr nicht:
// bei 0.06 liegt die Faust am Schwert in Ruhe gerade noch unter dem Bildrand (s. Test).
const FIST_BELOW_GRIP: f32 = 0.06;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

// Die Drehung, die die Richtung a auf b legt.
fn rotation_between(a: [f32; 3], b: [f32; 3]) -> Affine {
    let (a, b) = (norm(a), norm(b));
    let achse = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let sin = (achse[0] * achse[0] + achse[1] * achse[1] + achse[2] * achse[2]).sqrt();
    if sin < 1e-6 {
        return Affine::IDENTITY;
    }
    let cos = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    Affine::rotation_axis_angle(sin.atan2(cos), [achse[0] / sin, achse[1] / sin, achse[2] / sin])
}

// Die Ego-Faust an `griff` (Sichtraum ohne Nachschwingen, rechte Hand: links wird
// gespiegelt), der Unterarm zeigt von dort zur Schulter. Vor first_person_arm_transform(right,
// 0, 0) zu setzen, hinter das Nachschwingen. Der Arm folgt also nur der LAGE des Griffs, nicht
// seiner Drehung: dreht sich das Item (Ansehen), dreht es sich in der Hand wie um ein
// Handgelenk, statt den ganzen Unterarm mitzureissen.
#[must_use] 
pub fn arm_to(griff: [f32; 3], right: bool) -> Affine {
    arm_from(griff, shoulder(right), right)
}

// Der Arm mit der Faust bei `faust` (Sichtraum, schon gespiegelt), der Unterarm auf dem BILD um
// `neigung` Grad aus der Senkrechten nach innen gekippt -- gerechnet an der Stelle der Faust, mit
// der Perspektive (weit am Rand kippt ein gleich gedrehter Arm sonst sichtbar mit). Nach hinten
// in die Tiefe laeuft er wie die leere Hand. Vor first_person_arm_transform(right, 0, 0).
#[must_use] 
pub fn arm_on_screen(faust: [f32; 3], right: bool, neigung: f32) -> Affine {
    let s = if right { 1.0 } else { -1.0 };
    let [x, y, z] = faust;
    let (dy, dz) = (FOREARM[1], FOREARM[2]);
    // Bildrichtung eines Schritts d an der Faust: (dx*(-z) + x*dz, dy*(-z) + y*dz), bis auf 1/z^2.
    let bild_y = dy * -z + y * dz;
    let dx = (-s * neigung.to_radians().tan() * bild_y - x * dz) / -z;
    arm_from(faust, sub(faust, [dx, dy, dz]), right)
}

// Wie arm_to, nur mit einer anderen Schulter `schulter` (Sichtraum, schon gespiegelt): die
// Faust bleibt bei `griff`, der Unterarm zeigt zu ihr.
#[must_use] 
pub fn arm_from(griff: [f32; 3], schulter: [f32; 3], right: bool) -> Affine {
    let s = if right { 1.0 } else { -1.0 };
    let m = |v: [f32; 3]| [s * v[0], v[1], v[2]];
    let f = m(FIST);
    Affine::translation(griff).then(rotation_between(m(FOREARM), sub(griff, schulter))).then(Affine::translation([-f[0], -f[1], -f[2]]))
}

// Die Hand-Debug-Werte (s. Settings::hand_fist_items, rechte Hand), mit denen der Arm genau wie
// mit leerer Hand liegt, wenn die Faust ohne Werte bei `faust` saesse (fist_at): die Faust
// zurueck auf FIST, keine Drehung, und die Schulter so um sie gedreht, dass der Unterarm wie
// in Ruhe zeigt -- dann ist arm_from die Einheit. Die Schulter ganzzahlig in Grad gesucht.
#[must_use] 
pub fn empty_hand_tuning(faust: [f32; 3]) -> [i32; 16] {
    let mut w = [0; 16];
    for i in 0..3 {
        w[i] = ((FIST[i] - faust[i]) * 1000.0).round() as i32;
    }
    let von = norm(sub(SHOULDER, FIST));
    let soll = norm([-FOREARM[0], -FOREARM[1], -FOREARM[2]]);
    let mut best = (f32::MIN, 0, 0);
    for x in -180..180 {
        for y in -180..180 {
            let d = Affine::rotation_xyz_deg(x as f32, y as f32, 0.0).apply(von);
            let cos = d[0] * soll[0] + d[1] * soll[1] + d[2] * soll[2];
            if cos > best.0 {
                best = (cos, x, y);
            }
        }
    }
    w[7] = best.1;
    w[8] = best.2;
    w
}

// Die Faust der leeren Hand in Ruhe (FIST), links gespiegelt.
#[must_use] 
pub fn rest_fist(right: bool) -> [f32; 3] {
    let s = if right { 1.0 } else { -1.0 };
    [s * FIST[0], FIST[1], FIST[2]]
}

// empty_hand_tuning fuer die Faust in Ruhe, einmal gesucht (die Suche ist teuer).
pub fn rest_tuning() -> [i32; 16] {
    static WERTE: std::sync::OnceLock<[i32; 16]> = std::sync::OnceLock::new();
    *WERTE.get_or_init(|| empty_hand_tuning(FIST))
}

// Wo die Schulter im Sichtraum sitzt (s. arm_to), links gespiegelt.
#[must_use] 
pub fn shoulder(right: bool) -> [f32; 3] {
    let s = if right { 1.0 } else { -1.0 };
    [s * SHOULDER[0], SHOULDER[1], SHOULDER[2]]
}

// Laengsachse des Unterarms, wenn die Faust bei `faust` sitzt (s. arm_from): von der Schulter
// zur Faust. Sichtraum.
#[must_use] 
pub fn arm_axis(faust: [f32; 3], schulter: [f32; 3]) -> [f32; 3] {
    norm(sub(faust, schulter))
}

// Wo die Faust (FIST, s. arm_to) hin muss, damit sie das Item am Griffpunkt `griff` haelt:
// ein Stueck vom Griff zur Schulter hin, das Item steckt oben in ihr. Sichtraum ohne
// Nachschwingen, rechte Hand; links gespiegelt.
#[must_use] 
pub fn fist_at(griff: [f32; 3], right: bool) -> [f32; 3] {
    let s = if right { 1.0 } else { -1.0 };
    let zur_schulter = norm(sub([s * SHOULDER[0], SHOULDER[1], SHOULDER[2]], griff));
    std::array::from_fn(|i| griff[i] + FIST_BELOW_GRIP * zur_schulter[i])
}

fn rampe(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

// Ein- und Ausblenden der Haltung am Anfang und Ende.
fn blend(t: f32) -> f32 {
    rampe(t / 0.15).min(rampe((1.0 - t) / 0.15))
}

// Dritte Person (F5): der Arm hebt sich vor den Koerper (Drehung des Modellteils in Radiant,
// mit `blend` 0..1 ueber die Haltung der Figur zu legen), und das Item dreht sich an der Hand
// mit denselben Schluesselbildern wie in der Ego-Ansicht, nur ohne Verschiebung. Die
// Item-Drehung kommt hinter held_item::hand_pre_transform.
pub struct ThirdPerson {
    pub arm: [f32; 3],
    pub blend: f32,
    pub item: Affine,
    // Hochgeworfen (Kind::tossed): so hoch fliegt das Item ueber der Hand, in Bloecken senkrecht
    // in der Welt (sonst 0).
    pub lift: f32,
}

// Ein Ego-Wurf so hoch ist in F5 so viele Bloecke hoch (wie beim Jonglieren, die Figur steht
// weiter weg).
const THIRD_PERSON_LIFT: f32 = 1.6;

#[must_use] 
pub fn third_person(kind: Kind, t: f32, right: bool) -> ThirdPerson {
    if matches!(kind, Kind::Juggle(_)) {
        return ThirdPerson { arm: [0.0; 3], blend: 0.0, item: Affine::IDENTITY, lift: 0.0 };
    }
    let s = if right { 1.0 } else { -1.0 };
    let blend = blend(t);
    let (pos, rot, spin) = sample(kind.keys(), t);
    let ueber_hand = if kind == Kind::Mace { 0.0 } else { TY };
    let lift = if kind.tossed() { (pos[1] - ueber_hand).max(0.0) * THIRD_PERSON_LIFT } else { 0.0 };
    let (_, extra_rot) = extra(kind, t);
    let (x, y, z) = (rot[0] + extra_rot[0], rot[1] + extra_rot[1], rot[2] + extra_rot[2]);
    if kind == Kind::EmptyHand {
        // Waagerecht vorgestreckt ist Z die Laengsachse des Arms: die Handflaeche dreht nach
        // oben, das Fingerspiel zuckt mit.
        let arm = [-1.3 + x.to_radians(), s * -0.35, s * (-1.2 + z.to_radians())];
        return ThirdPerson { arm, blend, item: Affine::IDENTITY, lift };
    }
    let item = Affine::rotation_y_deg(s * y)
        .then(rotation_x_deg(x))
        .then(rotation_z_deg(s * z))
        .then(Affine::rotation_axis_angle((s * spin).to_radians(), [0.0, 1.0, 0.0]));
    ThirdPerson { arm: [-1.3, s * -0.35, 0.0], blend, item, lift }
}

#[cfg(test)]
mod tests {
    // Nah ansehen: voll herangeholt zeigt die flache Seite zur Kamera (bis auf das leichte
    // Hin- und Herdrehen der Schluesselbilder), auch wenn sie in der Hand schraeg lag.
    #[test]
    fn nah_ansehen_zeigt_die_flache_seite() {
        let mitte = [0.5, -0.4, -0.9];
        let normale = norm([0.8, 0.1, 0.4]);
        let m = close_look_transform(0.78, true, mitte, normale);
        let (a, b) = (m.apply(mitte), m.apply([mitte[0] + normale[0], mitte[1] + normale[1], mitte[2] + normale[2]]));
        let n = norm(sub(b, a));
        assert!(n[2].abs() > 0.97, "{n:?}");
    }

    // Werkzeug-Wurf: beim Vorlehnen bleibt der Griff in der Hand (nur um die Handlage versetzt),
    // die Spitze bewegt sich; ohne Sprung ueber die ganze Animation.
    #[test]
    fn werkzeug_kippt_um_den_griff() {
        // Im Flug (Abwurf 0.14 bis Fang 0.38) dreht es gleich schnell.
        let w = |t: f32| sample(TOOL_TOSS, t).1[0];
        let schritt = (w(0.38) - w(0.14)) / 24.0;
        for n in 0..24 {
            let t = 0.14 + n as f32 * 0.01;
            assert!((w(t + 0.01) - w(t) - schritt).abs() < 0.01, "t={t}");
        }
        let griff = [0.6, -0.6, -0.8];
        let mitte = [0.55, -0.3, -0.9];
        let lehnen = tool_toss_transform(Kind::Tool, 0.12, true, griff, mitte);
        let g = lehnen.apply(griff);
        let erwartet = [griff[0] + TX, griff[1] + TY - 0.045, griff[2]];
        assert!((0..3).all(|i| (g[i] - erwartet[i]).abs() < 0.02), "{g:?} statt {erwartet:?}");
        assert!((0..3).any(|i| (lehnen.apply(mitte)[i] - mitte[i]).abs() > 0.1));
        let mut vorher = tool_toss_transform(Kind::Tool, 0.0, true, griff, mitte).apply(mitte);
        for n in 1..=8000 {
            let jetzt = tool_toss_transform(Kind::Tool, n as f32 / 8000.0, true, griff, mitte).apply(mitte);
            assert!((0..3).all(|i| (jetzt[i] - vorher[i]).abs() < 0.01), "Sprung bei {n}: {vorher:?} -> {jetzt:?}");
            vorher = jetzt;
        }
    }

    use super::*;
    use crate::entity_model::first_person_item_transform;

    fn nah(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    // Anfang und Ende jeder Animation sind die gewohnte Haltung -- kein Sprung beim Start oder Ende.
    #[test]
    fn beginnt_und_endet_in_der_ruhelage() {
        let kinds = [
            Kind::Sword,
            Kind::Axe,
            Kind::Tool,
            Kind::Mace,
            Kind::Spear,
            Kind::Bow,
            Kind::Crossbow,
            Kind::Shield,
            Kind::Watch,
            Kind::Totem,
            Kind::FishingRod,
            Kind::Igniter,
            Kind::Block,
            Kind::Consumable,
            Kind::Other,
            Kind::EmptyHand,
            Kind::CloseLook,
            Kind::Throwable,
        ];
        let probe = [0.3, -0.2, -0.9];
        for kind in kinds {
            let keys = kind.keys();
            assert_eq!(keys[0].t, 0.0, "{kind:?}");
            assert_eq!(keys[keys.len() - 1].t, 1.0, "{kind:?}");
            assert!(keys.windows(2).all(|w| w[0].t < w[1].t), "{kind:?}: Zeiten nicht aufsteigend");
            for t in [0.0, 1.0] {
                for right in [true, false] {
                    assert!(nah(transform(kind, t, right).apply(probe), probe), "{kind:?} bei t={t}");
                    let tp = third_person(kind, t, right);
                    assert!(tp.blend == 0.0 && nah(tp.item.apply(probe), probe), "{kind:?} F5 bei t={t}");
                }
            }
        }
    }

    // In der flachen Phase zeigt die Klingenflaeche (ohne Inspect zur Bildmitte, -X) zur Kamera.
    #[test]
    fn schwert_zeigt_die_flache_seite() {
        let hand = first_person_item_transform(true, 0.0, 0.0);
        let normale = |m: Affine| {
            let (a, b) = (m.then(hand).apply([0.0; 3]), m.then(hand).apply([-1.0, 0.0, 0.0]));
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]]
        };
        assert!(normale(Affine::IDENTITY)[2].abs() < 0.01);
        let n = normale(transform(Kind::Sword, 0.6, true));
        assert!(n[2] > 0.9, "{n:?}");
    }

    // Griffpunkt durch eine firstperson_righthand-Haltung (wie HeldItems::first_person_grip).
    fn griff(rot: [f32; 3], trans: [f32; 3], scale: f32, punkt: [f32; 3]) -> [f32; 3] {
        Affine::translation([trans[0] / 16.0, trans[1] / 16.0, trans[2] / 16.0])
            .then(Affine::rotation_xyz_deg(rot[0], rot[1], rot[2]))
            .then(Affine::scale([scale; 3]))
            .then(Affine::translation([-0.5; 3]))
            .apply(punkt)
    }

    // Mit den Werten aus empty_hand_tuning liegt der Arm (fast) wie mit leerer Hand.
    #[test]
    fn leere_hand_werte_ergeben_den_ruhearm() {
        let faust = [0.95, -0.62, -0.84];
        let w = empty_hand_tuning(faust);
        let ziel = [faust[0] + w[0] as f32 / 1000.0, faust[1] + w[1] as f32 / 1000.0, faust[2] + w[2] as f32 / 1000.0];
        let schulter = Affine::translation(ziel)
            .then(Affine::rotation_xyz_deg(w[7] as f32, w[8] as f32, w[9] as f32))
            .then(Affine::translation([-ziel[0], -ziel[1], -ziel[2]]))
            .apply(shoulder(true));
        let m = arm_from(ziel, schulter, true);
        for p in [[0.0, 0.0, 0.0], FIST, [0.5, -1.5, 0.2]] {
            let q = m.apply(p);
            assert!((0..3).all(|i| (q[i] - p[i]).abs() < 0.03), "{p:?} -> {q:?} ({w:?})");
        }
    }

    // Die Faust sitzt am Griff, der Arm zeigt von dort zur Schulter; links gespiegelt.
    #[test]
    fn faust_haelt_den_griff() {
        let g = griff([0.0, -90.0, 25.0], [1.13, 3.2, 1.13], 0.68, [0.12, 0.12, 0.5]);
        for right in [true, false] {
            let s = if right { 1.0 } else { -1.0 };
            let ziel = [s * (ITEM_PIVOT[0] + g[0]), ITEM_PIVOT[1] + g[1], ITEM_PIVOT[2] + g[2]];
            let m = arm_to(ziel, right);
            let faust = m.apply([s * FIST[0], FIST[1], FIST[2]]);
            assert!(nah(faust, ziel), "{right}: {faust:?}");
            let unten = m.apply([s * (FIST[0] - FOREARM[0]), FIST[1] - FOREARM[1], FIST[2] - FOREARM[2]]);
            let soll = norm(sub([s * SHOULDER[0], SHOULDER[1], SHOULDER[2]], ziel));
            assert!(nah(norm(sub(unten, faust)), soll), "{right}");
        }
        // Die Faust sitzt knapp hinter dem Griff, zur Schulter hin.
        let griff_sicht = [ITEM_PIVOT[0] + g[0], ITEM_PIVOT[1] + g[1], ITEM_PIVOT[2] + g[2]];
        let f = fist_at(griff_sicht, true);
        let abstand = sub(f, griff_sicht);
        assert!(((abstand[0] * abstand[0] + abstand[1] * abstand[1] + abstand[2] * abstand[2]).sqrt() - FIST_BELOW_GRIP).abs() < 1e-4);
    }

    // In Ruhe liegt KEINE Ecke des Arms im Bild (Handprojektion: 70 Grad, 16:9, s.
    // render::hand_projection) -- Schwert, Armbrust, Bloecke. Hoeher sitzende flache Items
    // (Totem, Essen) zeigen die Faust bewusst: sie haelt das Item immer.
    #[test]
    fn arm_in_ruhe_unsichtbar() {
        use crate::entity_model::{first_person_arm_pose, first_person_arm_transform, player_arm_model};
        let griffe = [
            griff([0.0, -90.0, 25.0], [1.13, 3.2, 1.13], 0.68, [0.12, 0.12, 0.5]),
            griff([-90.0, 0.0, -55.0], [1.13, 3.2, 1.13], 0.68, [0.85, 0.15, 0.4]),
            griff([0.0, 45.0, 0.0], [0.0, 0.0, 0.0], 0.4, [0.5, -0.3, 0.5]),
        ];
        let tan = 35f32.to_radians().tan();
        for g in griffe {
            for slim in [false, true] {
                let ziel = fist_at([ITEM_PIVOT[0] + g[0], ITEM_PIVOT[1] + g[1], ITEM_PIVOT[2] + g[2]], true);
                let base = arm_to(ziel, true).then(first_person_arm_transform(true, 0.0, 0.0));
                let mut v = Vec::new();
                crate::emit_entity_model(&player_arm_model(slim, true), &first_person_arm_pose(true), base, [0.0, 0.0, 1.0, 1.0], 1.0, &mut v);
                for p in v.iter().map(|x| x.position) {
                    assert!(p[2] < -0.01, "{g:?}: Ecke hinter der Kamera {p:?}");
                    let (nx, ny) = (p[0] / -p[2] / tan / (16.0 / 9.0), p[1] / -p[2] / tan);
                    assert!(nx > 1.0 || ny < -1.0, "{g:?} slim={slim}: Ecke im Bild {p:?} ({nx:.2}, {ny:.2})");
                }
            }
        }
    }

    #[test]
    fn karte_und_fernrohr_ohne_inspect() {
        assert_eq!(kind("filled_map", false, false), None);
        assert_eq!(kind("spyglass", false, false), None);
        assert_eq!(kind("diamond_sword", false, false), Some(Kind::Sword));
        assert_eq!(kind("netherite_pickaxe", false, false), Some(Kind::Tool));
        assert_eq!(kind("iron_axe", false, false), Some(Kind::Axe));
        assert_eq!(kind("golden_apple", false, true), Some(Kind::Consumable));
        assert_eq!(kind("stone", true, false), Some(Kind::Block));
        assert_eq!(kind("stick", false, false), Some(Kind::Other));
        assert_eq!(kind("splash_potion", false, false), Some(Kind::Throwable));
        assert_eq!(kind("potion", false, true), Some(Kind::Throwable));
        assert_eq!(kind("ender_pearl", false, false), Some(Kind::Throwable));
    }

    // Die leere Hand lehnt auf dem Bild etwa 10 Grad nach innen; arm_on_screen haelt die
    // gewuenschte Neigung auch weit am Bildrand und tief unten.
    #[test]
    fn arm_lehnt_auf_dem_bild() {
        let bild = |p: [f32; 3]| [p[0] / -p[2], p[1] / -p[2]];
        let neigung = |m: Affine, right: bool| {
            let s = if right { 1.0 } else { -1.0 };
            let faust = m.apply([s * FIST[0], FIST[1], FIST[2]]);
            let ell = m.apply([s * (FIST[0] - 0.05 * FOREARM[0]), FIST[1] - 0.05 * FOREARM[1], FIST[2] - 0.05 * FOREARM[2]]);
            let (a, b) = (bild(ell), bild(faust));
            (-s * (b[0] - a[0])).atan2(b[1] - a[1]).to_degrees()
        };
        let leer = neigung(Affine::IDENTITY, true);
        assert!((5.0..15.0).contains(&leer), "{leer}");
        for right in [true, false] {
            let s = if right { 1.0 } else { -1.0 };
            for faust in [[0.93, -0.5, -1.01], [0.8, -0.85, -1.01], [0.3, -0.4, -1.0]] {
                let n = neigung(arm_on_screen([s * faust[0], faust[1], faust[2]], right, 11.0), right);
                assert!((n - 11.0).abs() < 0.5, "{right} {faust:?}: {n}");
            }
        }
    }


    // Am echten Armmodell gemessen: die Neigung auf dem Bild (Grad, + = nach innen) vom
    // Schulterende zum Handende -- leere Hand rund 10, beim Jonglieren die eingestellte.
    #[test]
    fn arm_modell_neigt_nach_innen() {
        let bild = |p: [f32; 3]| [p[0] / -p[2], p[1] / -p[2]];
        let miss = |arm: Affine, right: bool| {
            let s = if right { 1.0 } else { -1.0 };
            let model = crate::player_arm_model(false, right);
            let base = arm.then(crate::first_person_arm_transform(right, 0.0, 0.0));
            let teil = if right { "right_arm" } else { "left_arm" };
            let m = crate::entity_model::part_transform(&model, &crate::first_person_arm_pose(right), base, teil).unwrap();
            let (a, b) = (bild(m.apply([-s / 16.0, -2.0 / 16.0, 0.0])), bild(m.apply([-s / 16.0, 10.0 / 16.0, 0.0])));
            (-s * (b[0] - a[0])).atan2(b[1] - a[1]).to_degrees()
        };
        for right in [true, false] {
            let leer = miss(Affine::IDENTITY, right);
            assert!((9.0..12.0).contains(&leer), "{right}: {leer}");
            for f in [[0.93, -0.5, -1.01], [0.86, -0.82, -1.01]] {
                let s = if right { 1.0 } else { -1.0 };
                let n = miss(arm_on_screen([s * f[0], f[1], f[2]], right, 11.5), right);
                assert!((n - 11.5).abs() < 0.5, "{right} {f:?}: {n}");
            }
        }
    }

}

