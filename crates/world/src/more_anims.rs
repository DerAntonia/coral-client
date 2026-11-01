// Mehr Animationen (Video-Tab, Abschnitt Animationen): Haltungen fuer Spielerfiguren in der dritten Person, die Vanilla
// nur in der Ich-Sicht zeigt oder gar nicht -- essen, Karte lesen, Fackel hochhalten, an der
// Leiter klettern, rudern, die Zuegel halten, brennend mit den Armen wedeln ...
//
// Angeregt von der Mod "Not Enough Animations" (tr7zw): dieselbe Idee, eigene Umsetzung. Die
// Schicht laeuft NACH Vanillas Haltung (entity_model::biped_pose_full) und ueberschreibt
// einzelne Gliedmassen. Je Gliedmass gewinnt genau eine Animation, die mit dem hoechsten
// Rang; was Vanilla selbst an einem Arm zeigt (Schild, Speer, Schlag), bleibt unangetastet.
//
// Alle Winkel wie in Vanilla: Radiant, pitch/yaw/roll, fuer den RECHTEN Arm angegeben. Der
// linke bekommt sie gespiegelt (Gierung und Rollen mit umgekehrtem Vorzeichen).

use crate::entity_model::Pose;

// Die Schalter, als Bits (s. Settings::anim_flags in der App).
pub const EAT_DRINK: u32 = 1 << 0;
pub const HOLD_UP: u32 = 1 << 1;
pub const MAP: u32 = 1 << 2;
pub const LADDER: u32 = 1 << 3;
pub const CRAWL: u32 = 1 << 4;
pub const BURNING: u32 = 1 << 5;
pub const FREEZING: u32 = 1 << 6;
pub const ITEM_SWAP: u32 = 1 << 7;
pub const ELYTRA: u32 = 1 << 8;
pub const HORSE: u32 = 1 << 9;
pub const BOAT: u32 = 1 << 10;
pub const SMOOTH: u32 = 1 << 11;
pub const BED: u32 = 1 << 12;
pub const PET: u32 = 1 << 13;
pub const LANTERN: u32 = 1 << 14;
pub const HIDE_OFFHAND: u32 = 1 << 15;
pub const BODY_TURN: u32 = 1 << 16;
pub const SWORD: u32 = 1 << 17;
pub const FALLING: u32 = 1 << 18;
pub const HUG: u32 = 1 << 19;
pub const NARUTO: u32 = 1 << 20;
pub const CUSTOM_BOW: u32 = 1 << 21;
pub const CLAMP_CROSSBOW: u32 = 1 << 22;
pub const HORSE_LEGS: u32 = 1 << 23;
pub const ALL: u32 = (1 << 24) - 1;
// Was ohne Einstellung aus ist: Brennen und alles, was auch die Mod nur auf Wunsch zeigt.
pub const DEFAULT_OFF: u32 = BURNING | SWORD | FALLING | HUG | NARUTO | CUSTOM_BOW | CLAMP_CROSSBOW | HORSE_LEGS;
pub const DEFAULT: u32 = ALL & !DEFAULT_OFF;

// Die Schalter in der Reihenfolge des Menues, mit Namen.
pub const FEATURES: [(u32, &str); 24] = [
    (EAT_DRINK, "Essen & Trinken"),
    (BODY_TURN, "Beim Essen und Blocken zum Blick drehen"),
    (HOLD_UP, "Fackel, Laterne, Kompass, Uhr hochhalten"),
    (LANTERN, "Laternen pendeln"),
    (MAP, "Karte halten"),
    (HIDE_OFFHAND, "Nebenhand beim Bogen verstecken"),
    (LADDER, "Leiter klettern"),
    (CRAWL, "Kriechen"),
    (BURNING, "Brennen"),
    (FREEZING, "Frieren"),
    (ITEM_SWAP, "Hände tauschen"),
    (ELYTRA, "Elytra"),
    (HORSE, "Zügel halten"),
    (HORSE_LEGS, "Beine beim Reiten"),
    (BOAT, "Rudern"),
    (BED, "Ruhig im Bett"),
    (PET, "Haustiere streicheln"),
    (HUG, "Umarmen"),
    (FALLING, "Fallen"),
    (NARUTO, "Naruto-Rennen"),
    (CUSTOM_BOW, "Bogen schräg halten"),
    (CLAMP_CROSSBOW, "Armbrust tiefer halten"),
    (SWORD, "Schwert auf dem Rücken"),
    (SMOOTH, "Weiche Übergänge"),
];

// Wie lange der Handtausch die Arme zusammenfuehrt, in Ticks.
pub const SWAP_TICKS: u8 = 10;

// Was in der Hand nach oben vor den Blick gehalten wird: Kompass und Uhr zum Ablesen, Fackeln
// und Laternen zum Leuchten.
#[must_use] 
pub fn held_up(item_name: &str) -> bool {
    matches!(item_name, "clock" | "compass" | "recovery_compass") || item_name.ends_with("torch") || is_lantern(item_name)
}

// Eine Laterne zum Tragen (nicht die Kuerbis- oder Seelaterne, das sind Bloecke ohne Henkel).
#[must_use] 
pub fn is_lantern(item_name: &str) -> bool {
    item_name.ends_with("lantern") && !matches!(item_name, "jack_o_lantern" | "sea_lantern")
}

// Das Blockmodell einer Laterne: gewachste teilen sich das Modell der ungewachsten.
#[must_use] 
pub fn lantern_block_model(item_name: &str) -> Option<String> {
    is_lantern(item_name).then(|| format!("block/{}", item_name.strip_prefix("waxed_").unwrap_or(item_name)))
}

// Ein Schwert, das man auf dem Ruecken traegt, wenn man es gerade nicht in der Hand hat.
#[must_use] 
pub fn is_sword(item_name: &str) -> bool {
    item_name.ends_with("_sword")
}

// Die Pendelbewegung einer Laterne in der Hand: die Laterne haengt an einer Feder hinter der
// Hand her, aus ihrer Geschwindigkeit (vor/zurueck und seitlich zur Blickrichtung) ergeben
// sich die beiden Kippwinkel. Je Spielerfigur eine.
#[derive(Clone, Copy, Debug, Default)]
pub struct LanternSwing {
    tick: Option<i64>,
    hand: Option<[f64; 3]>,
    pos: [f64; 3],
    vel: [f64; 3],
    last_vel: [f64; 3],
}

impl LanternSwing {
    // `hand`: wo die Laterne haengt (Welt), `tick`: ganzzahliger Tick der Figur (gerechnet wird
    // einmal je Tick), `delta`: Zwischentick-Anteil, `yaw`/`pitch`: Blick in Grad. Liefert die
    // Kippung um X und Z in Grad.
    pub fn update(&mut self, hand: [f64; 3], tick: i64, delta: f32, yaw: f32, pitch: f32) -> (f32, f32) {
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let len2 = |a: [f64; 3]| a[0] * a[0] + a[1] * a[1] + a[2] * a[2];
        // Die Hand selbst wird geglaettet, damit kleine Ruckler der Animation nicht pendeln.
        let geglaettet = match self.hand {
            Some(alt) if len2(sub(hand, alt)) <= 4.0 => [0, 1, 2].map(|i| alt[i] + (hand[i] - alt[i]) * 0.3),
            _ => {
                self.pos = hand;
                hand
            }
        };
        self.hand = Some(geglaettet);
        if self.tick != Some(tick) {
            self.tick = Some(tick);
            let weg = sub(self.pos, geglaettet);
            let zug = weg.map(|w| w * -0.15);
            self.last_vel = self.vel;
            if len2(weg) > 100.0 {
                self.vel = zug;
                self.pos = geglaettet;
            } else {
                self.vel = [0, 1, 2].map(|i| (self.vel[i] + zug[i]) * 0.95);
                self.pos = [0, 1, 2].map(|i| self.pos[i] + self.vel[i]);
            }
        }
        let d = f64::from(delta.clamp(0.0, 1.0));
        let v = [0, 1, 2].map(|i| self.last_vel[i] + (self.vel[i] - self.last_vel[i]) * d);
        let (y, p) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
        let vorn = [-y.sin(), 0.0, y.cos()];
        let rechts = [vorn[2], 0.0, -vorn[0]];
        let vor = v[0] * vorn[0] + v[2] * vorn[2] - v[1] * p.cos() * 0.6;
        let seit = v[0] * rechts[0] + v[2] * rechts[2];
        let x = (vor as f32 * 90.0).clamp(-90.0, 90.0) - pitch * 0.25;
        let z = (-seit as f32 * 90.0).clamp(-90.0, 90.0);
        (x, z)
    }
}

#[must_use] 
pub fn is_map(item_name: &str) -> bool {
    item_name == "filled_map"
}

// Wie eine Karte gehalten wird.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MapHold {
    #[default]
    None,
    // In der Haupthand bei leerer Nebenhand: mit beiden Haenden vor die Brust.
    Both,
    // Nur ein Arm (true = rechts).
    Arm(bool),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    // Welche Schalter an sind (s. EAT_DRINK ...). 0 = alle aus.
    pub features: u32,
    // Blickneigung in Grad, Vanilla-Konvention (positiv = nach unten).
    pub pitch_deg: f32,
    // Entity-Alter in Ticks samt Zwischentick-Anteil.
    pub age: f32,
    // Fortschritt der Schrittanimation (limbSwing).
    pub limb: f32,
    // Schlaegt gerade ein Arm, und wenn ja: der rechte?
    pub swinging: Option<bool>,
    // Arme, die Vanilla schon belegt (Schild, Speer): (rechts, links).
    pub taken: (bool, bool),
    // Isst oder trinkt die Figur: (rechter Arm?, Benutzungsdauer in Ticks).
    pub eating: Option<(bool, f32)>,
    // Haelt der Arm etwas, das hochgehalten wird (s. held_up): (rechts, links).
    pub hold_up: (bool, bool),
    pub map: MapHold,
    // Klettert an einer Leiter: Hoehe der Fuesse in Bloecken (treibt den Kletterschritt).
    pub ladder: Option<f64>,
    // Kriecht an Land (Schwimmhaltung ausserhalb vom Wasser) -- die Bewegung selbst steckt in
    // biped_pose_full (BipedExtras::crawl), hier zaehlt nur, dass die Gliedmassen belegt sind.
    pub crawling: bool,
    pub burning: bool,
    pub freezing: bool,
    // Laufender Handtausch: 1 am Anfang, 0 am Ende.
    pub swap: Option<f32>,
    // Gleitet mit Elytra: Geschwindigkeit in Bloecken pro Tick.
    pub glide_velocity: Option<[f32; 3]>,
    // Lenkt ein Pferd: dessen Laufphase.
    pub horse: Option<f32>,
    // Lenkt ein Boot: Ruderzeit links, rechts (s. Entity::rowing_time).
    pub boat: Option<[f32; 2]>,
    // Schlaeft im Bett.
    pub sleeping: bool,
    // Streichelt schleichend einen Wolf oder eine Katze: mit dem Arm der Haupthand (rechts?).
    pub petting: Option<bool>,
    // Schleicht vor einem ebenfalls schleichenden Spieler.
    pub hugging: bool,
    pub sprinting: bool,
    // Faellt: wie schnell (Bloecke je Tick, durch 3.5), ab 1/7 sichtbar.
    pub fall_speed: Option<f32>,
    // Spannt einen Bogen: der Bogenarm (rechts?).
    pub bow_arm: Option<bool>,
    // Spannt oder haelt eine Armbrust im Anschlag: der Armbrustarm (rechts?).
    pub crossbow_arm: Option<bool>,
    // Haelt sich schleichend still an der Leiter: dieser Arm (rechts?) bleibt frei.
    pub ladder_free: Option<bool>,
    // Schwimmt im Wasser, stirbt, Riptide: dann zeigt die Figur nur Vanilla.
    pub vanilla_only: bool,
}

impl Input {
    const fn on(&self, flag: u32) -> bool {
        self.features & flag != 0
    }
}

const fn arm(right: bool) -> &'static str {
    if right { "right_arm" } else { "left_arm" }
}

const fn leg(right: bool) -> &'static str {
    if right { "right_leg" } else { "left_leg" }
}

// Winkel fuer die rechte Seite setzen, links gespiegelt. Der Drehpunkt bleibt (Schleichen).
fn set_mirrored(pose: &mut Pose, part: &'static str, right: bool, [pitch, yaw, roll]: [f32; 3]) {
    let s = if right { 1.0 } else { -1.0 };
    pose.rotate(part, [pitch, yaw * s, roll * s]);
}

// Wie weit nach unten geschaut wird: 0 ganz oben, 1 ganz unten.
fn look_down(pitch_deg: f32) -> f32 {
    ((pitch_deg + 90.0) / 180.0).clamp(0.0, 1.0)
}

fn mix(t: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * t
}

// Ein Zitterwert in -1..1, der sich jeden Frame aendert, aber ohne Zufallsquelle auskommt.
fn shiver(age: f32, salt: f32) -> f32 {
    ((age * 91.7 + salt).sin() * 43_758.547).fract() * 2.0 - 1.0
}

// Die Schicht anwenden. `pose` ist Vanillas fertige Haltung.
pub fn apply(pose: &mut Pose, input: &Input) {
    if input.features == 0 {
        return;
    }
    if input.sleeping {
        // Im Bett liegen die Arme ruhig am Koerper, statt im Schlaf weiterzuwippen.
        if input.on(BED) {
            pose.rotate("right_arm", [0.0; 3]);
            pose.rotate("left_arm", [0.0; 3]);
        }
        return;
    }
    if input.vanilla_only {
        return;
    }
    let kopf = pose.get("head").rotation;

    if let Some(v) = input.glide_velocity {
        // Im Flug gehoert die ganze Figur dem Gleiten -- auch wenn der Schalter aus ist, dann
        // eben in Vanillas Haltung.
        if input.on(ELYTRA) {
            apply_glide(pose, input.limb, v);
        }
        return;
    }

    // Bogen und Armbrust sind Vanillas Haltungen; diese beiden Schalter aendern sie ab.
    if let (true, Some(rechts)) = (input.on(CUSTOM_BOW), input.bow_arm) {
        apply_custom_bow(pose, rechts, kopf);
    }
    if let (true, Some(_)) = (input.on(CLAMP_CROSSBOW), input.crossbow_arm) {
        for teil in ["right_arm", "left_arm"] {
            let mut r = pose.get(teil).rotation;
            r[0] = wrap(r[0]).clamp(-1.75, 0.0);
            pose.rotate(teil, r);
        }
    }

    // Je Arm: belegt Vanilla ihn, oder schlaegt er gerade, bleibt er, wie er ist. Der Schlag
    // zaehlt nur bei den Animationen, die ihn zulassen (s. unten) -- Essen und Tauschen gehen vor.
    for right in [true, false] {
        let vanilla = if right { input.taken.0 } else { input.taken.1 };
        if vanilla {
            continue;
        }
        let schlaegt = input.swinging == Some(right);
        let teil = arm(right);
        if let (true, Some(f)) = (input.on(ITEM_SWAP), input.swap) {
            // Die Arme schwingen vor dem Bauch zusammen und gehen dann zurueck.
            set_mirrored(pose, teil, right, [-0.5, 0.2, -f]);
        } else if let (true, Some((rechts, ticks))) = (input.on(EAT_DRINK), input.eating) {
            if rechts == right {
                // Zum Mund, je nach Blick hoeher oder tiefer, mit kleinem Kau-Wippen.
                let wippen = ((ticks / 4.0) * std::f32::consts::PI).cos().abs() * 0.2;
                set_mirrored(pose, teil, right, [-mix(1.0 - look_down(input.pitch_deg), 1.0, 2.0) + wippen, -0.3, 0.3]);
            } else {
                arm_low(pose, input, right, schlaegt, kopf);
            }
        } else if input.on(HUG) && input.hugging {
            set_mirrored(pose, teil, right, [if right { -1.5 } else { -1.0 }, -0.2, 0.3]);
        } else if let (true, Some(haupt)) = (input.on(PET), input.petting) {
            if haupt == right {
                // Ueber den Kopf des Tiers streichen: der Arm wiegt seitlich hin und her.
                let streichen = (input.age * 50.0 / 60.0).sin() * 0.2;
                set_mirrored(pose, teil, right, [-mix(1.0 - look_down(input.pitch_deg), 1.0, 2.0), -0.6, 0.3 + streichen]);
            } else {
                arm_low(pose, input, right, schlaegt, kopf);
            }
        } else if let (true, Some(gang)) = (input.on(HORSE), input.horse) {
            if !schlaegt {
                let zug = -(gang * 0.3).cos() * 0.1;
                set_mirrored(pose, teil, right, [-1.1 - zug, -0.2, 0.3]);
            }
        } else if let (true, Some(ruder)) = (input.on(BOAT), input.boat) {
            if !schlaegt {
                let zeit = if right { ruder[1] } else { ruder[0] };
                set_mirrored(pose, teil, right, [-1.1 - zeit.sin() * 0.3, 0.2, 0.3]);
            }
        } else {
            arm_low(pose, input, right, schlaegt, kopf);
        }
    }

    // Die Beine: Reiten (gespreizt am Pferd herab), Leiter, Fallen.
    if let (true, true, Some(_)) = (input.on(HORSE), input.on(HORSE_LEGS), input.horse) {
        for right in [true, false] {
            set_mirrored(pose, leg(right), right, [-0.2, 0.0, 0.8]);
        }
    } else if let (true, Some(y)) = (input.on(LADDER), input.ladder) {
        for right in [true, false] {
            let s = if right { 1.0 } else { -1.0 };
            let schritt = -((y * 2.0) as f32).cos() * 0.35 * s;
            set_mirrored(pose, leg(right), right, [-1.0 - schritt, -0.2, 0.3]);
        }
    } else if let (true, Some(v)) = (input.on(FALLING), input.fall_speed) {
        let staerke = v.min(1.0);
        for right in [true, false] {
            let phase = if right { 0.0 } else { std::f32::consts::PI };
            let mut r = pose.get(leg(right)).rotation;
            r[0] = (input.age * 0.6662 + phase).cos() * 1.4 * staerke;
            r[2] = 0.6 * staerke * if right { 1.0 } else { -1.0 };
            pose.rotate(leg(right), r);
        }
    }

    if input.on(BURNING) && input.burning && input.ladder.is_none() {
        let mut k = pose.get("head").rotation;
        k[1] += input.age.sin() * 0.1;
        pose.rotate("head", k);
    }
}

// Was ein Arm bekommt, wenn keine der vorrangigen Animationen (Tausch, Essen, Reiten) greift:
// Leiter, Naruto, Fallen, Brennen/Frieren, dann Karte und Hochhalten.
fn arm_low(pose: &mut Pose, input: &Input, right: bool, schlaegt: bool, kopf: [f32; 3]) {
    let teil = arm(right);
    let s = if right { 1.0 } else { -1.0 };
    if let (true, Some(y)) = (input.on(LADDER), input.ladder) {
        if input.ladder_free != Some(right) {
            if !schlaegt {
                let schritt = -((y * 2.0) as f32).cos() * 0.35 * s;
                set_mirrored(pose, teil, right, [-1.7 - schritt, -0.2, 0.3]);
            }
            return;
        }
    }
    if input.on(NARUTO) && input.sprinting {
        if !schlaegt {
            // Die Arme gestreckt nach hinten.
            set_mirrored(pose, teil, right, [1.0, -0.2, 0.3]);
        }
        return;
    }
    if let (true, Some(v)) = (input.on(FALLING), input.fall_speed) {
        // Mit ausgebreiteten Armen rudernd, je schneller, desto weiter.
        let staerke = v.min(1.0);
        let phase = if right { std::f32::consts::PI } else { 0.0 };
        let mut r = pose.get(teil).rotation;
        r[0] = (input.age * 0.6662 + phase).cos() * staerke;
        r[2] = 1.9 * (v * 2.0).min(1.0) * s;
        pose.rotate(teil, r);
        return;
    }
    if input.on(BURNING) && input.burning {
        if !schlaegt {
            // Arme hoch ueber den Kopf und wedeln.
            let wedeln = input.age.sin() * 0.1 * s;
            set_mirrored(pose, teil, right, [-2.6 + wedeln, -0.2, -0.3]);
        }
        return;
    }
    if input.on(FREEZING) && input.freezing {
        if !schlaegt {
            // Die Arme um den Koerper geschlungen, zitternd.
            let zittern = shiver(input.age, if right { 0.0 } else { 7.0 }) * 0.05;
            let rollen = if right { -0.95 } else { -1.25 };
            set_mirrored(pose, teil, right, [if right { -0.5 } else { -0.6 }, 0.2, rollen + zittern]);
        }
        return;
    }
    if input.crawling && input.on(CRAWL) {
        return;
    }
    if input.on(MAP) {
        let unten = look_down(input.pitch_deg);
        match input.map {
            MapHold::Both => {
                set_mirrored(pose, teil, right, [-mix(1.0 - unten, 0.7, 0.9), mix(1.0 - unten, -0.3, -0.2), 0.3]);
                return;
            }
            MapHold::Arm(r) if r == right && !schlaegt => {
                set_mirrored(pose, teil, right, [-mix(1.0 - unten, 0.5, 1.5), 0.0, 0.3]);
                return;
            }
            _ => {}
        }
    }
    let hoch = if right { input.hold_up.0 } else { input.hold_up.1 };
    if input.on(HOLD_UP) && hoch && !schlaegt {
        // Vor den Blick: der Arm folgt Kopfneigung und -drehung, gedeckelt.
        let pitch = (-std::f32::consts::FRAC_PI_2 + kopf[0]).clamp(-2.5, 0.0);
        let yaw = (0.1 + kopf[1] * s).clamp(-0.2, 0.2);
        set_mirrored(pose, teil, right, [pitch, yaw, 0.1]);
    }
}

// Der Bogen schraeg vor dem Koerper statt waagerecht vor dem Gesicht: beide Arme um 1.5 gerollt,
// Neigung und Gierung aus dem Blick getauscht (so kippt der Bogen beim Hoch- und Runterzielen
// zur Seite).
fn apply_custom_bow(pose: &mut Pose, bogen_rechts: bool, kopf: [f32; 3]) {
    let inv = if bogen_rechts { 1.0 } else { -1.0 };
    let (bogen, zug) = if bogen_rechts { ("right_arm", "left_arm") } else { ("left_arm", "right_arm") };
    let mut b = pose.get(bogen).rotation;
    b[1] = inv * (-0.1 + wrap(-kopf[0])).clamp(-1.25, 0.5);
    b[0] = (-std::f32::consts::FRAC_PI_2 + inv * wrap(kopf[1])).clamp(-2.0, 0.0);
    b[2] += inv * 1.5;
    pose.rotate(bogen, b);
    let mut z = pose.get(zug).rotation;
    z[1] = inv * (0.1 + wrap(-kopf[0])).clamp(-1.05, 0.7);
    z[0] = (-std::f32::consts::FRAC_PI_2 + inv * wrap(kopf[1]) + 0.8).clamp(-1.05, -0.65);
    z[2] += inv * 1.5;
    pose.rotate(zug, z);
}

// Gleitflug: Arme und Beine leicht gespreizt, der Schwung waechst, je langsamer man fliegt.
fn apply_glide(pose: &mut Pose, limb: f32, v: [f32; 3]) {
    let mut k = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) / 0.2;
    k = (k * k * k).max(1.0);
    let spreizen = (0.1507964 / k).clamp(0.1, 0.25);
    let pi = std::f32::consts::PI;
    for (teil, phase, weite, rollen) in [
        ("left_arm", 0.0, 0.5, -spreizen),
        ("right_arm", pi, 0.5, spreizen),
        ("left_leg", pi, 0.7, -spreizen),
        ("right_leg", 0.0, 0.7, spreizen),
    ] {
        let mut r = pose.get(teil).rotation;
        r[0] = (limb * 0.6662 + phase).cos() * weite / k;
        r[2] = rollen;
        pose.rotate(teil, r);
    }
}

// Merkt sich die Hand-Items und erkennt den Tausch (Taste F): Haupt- und Nebenhand haben ihre
// Items getauscht. Je Spielerfigur eine, getickt mit dem Entity.
#[derive(Clone, Copy, Debug, Default)]
pub struct SwapWatch {
    last: Option<(u32, u32)>,
    ticks: u8,
}

impl SwapWatch {
    // Ein Tick mit den Item-IDs (0 = leer) in Haupt- und Nebenhand.
    pub const fn tick(&mut self, main: u32, off: u32) {
        if let Some((alt_haupt, alt_neben)) = self.last {
            if (main != 0 || off != 0) && alt_haupt != alt_neben && alt_haupt == off && alt_neben == main {
                self.ticks = SWAP_TICKS;
            }
        }
        self.last = Some((main, off));
        self.ticks = self.ticks.saturating_sub(1);
    }

    // Wie weit der Tausch noch laeuft: 1 am Anfang, 0 am Ende; None, wenn keiner laeuft.
    #[must_use] 
    pub fn progress(&self, tick_progress: f32) -> Option<f32> {
        (self.ticks > 0).then(|| ((f32::from(self.ticks) + 1.0 - tick_progress.clamp(0.0, 1.0)) / f32::from(SWAP_TICKS)).clamp(0.0, 1.0))
    }
}

// Die Glaettung: Arme und Beine laufen ihrer Zielhaltung hinterher, statt zu springen -- beim
// Wechsel zwischen zwei Haltungen (Essen beginnt, Fackel kommt in die Hand) gleiten sie
// hinueber. Je Tick holt sie 90 % des Abstands auf, also nach ein, zwei Ticks praktisch da.
#[derive(Clone, Copy, Debug, Default)]
pub struct Smoothing {
    stand: Option<[[f32; 3]; 4]>,
}

const SMOOTHED: [&str; 4] = ["right_arm", "left_arm", "right_leg", "left_leg"];
// Rest des Abstands nach einem Tick (50 ms).
const SMOOTH_REST_PER_TICK: f32 = 0.1;

impl Smoothing {
    // `dt` Sekunden seit dem letzten Bild dieser Figur. `snap`: diese Teile (Reihenfolge wie
    // SMOOTHED) springen sofort -- ein gehobenes Schild muss man im selben Bild sehen.
    pub fn apply(&mut self, pose: &mut Pose, dt: f32, snap: [bool; 4]) {
        let rest = SMOOTH_REST_PER_TICK.powf((dt / 0.05).clamp(0.0, 20.0));
        let mut neu = [[0.0; 3]; 4];
        for (i, teil) in SMOOTHED.iter().enumerate() {
            let ziel = pose.get(teil).rotation;
            neu[i] = match self.stand {
                Some(alt) if !snap[i] && dt < 0.5 => {
                    let mut r = [0.0; 3];
                    for a in 0..3 {
                        r[a] = ziel[a] + wrap(alt[i][a] - ziel[a]) * rest;
                    }
                    r
                }
                _ => ziel,
            };
            pose.rotate(teil, neu[i]);
        }
        self.stand = Some(neu);
    }
}

// Winkelabstand auf -PI..PI, damit nie der lange Weg um den Kreis genommen wird.
fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basis() -> Pose {
        crate::entity_model::biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0)
    }

    #[test]
    fn modul_aus_aendert_nichts() {
        let mut pose = basis();
        let vorher = pose.get("right_arm").rotation;
        apply(&mut pose, &Input { eating: Some((true, 3.0)), ..Default::default() });
        assert_eq!(pose.get("right_arm").rotation, vorher);
    }

    #[test]
    fn essen_hebt_den_essarm_zum_mund() {
        let mut pose = basis();
        apply(&mut pose, &Input { features: ALL, eating: Some((true, 0.0)), ..Default::default() });
        assert!(pose.get("right_arm").rotation[0] < -1.0);
        assert!(pose.get("left_arm").rotation[0].abs() < 0.2);
    }

    #[test]
    fn links_ist_gespiegelt() {
        let mut pose = basis();
        apply(&mut pose, &Input { features: ALL, hold_up: (true, true), ..Default::default() });
        let (r, l) = (pose.get("right_arm").rotation, pose.get("left_arm").rotation);
        assert!((r[0] - l[0]).abs() < 1e-6);
        assert!((r[1] + l[1]).abs() < 1e-6);
        assert!((r[2] + l[2]).abs() < 1e-6);
    }

    #[test]
    fn schild_und_schlag_bleiben_vanilla() {
        let mut pose = basis();
        let vorher = pose.get("right_arm").rotation;
        apply(&mut pose, &Input { features: ALL, taken: (true, false), hold_up: (true, false), ..Default::default() });
        assert_eq!(pose.get("right_arm").rotation, vorher);
        apply(&mut pose, &Input { features: ALL, swinging: Some(true), hold_up: (true, false), ..Default::default() });
        assert_eq!(pose.get("right_arm").rotation, vorher);
    }

    #[test]
    fn tausch_wird_erkannt_und_laeuft_aus() {
        let mut w = SwapWatch::default();
        w.tick(5, 9);
        assert!(w.progress(0.0).is_none());
        w.tick(9, 5);
        assert!(w.progress(0.0).is_some());
        for _ in 0..SWAP_TICKS {
            w.tick(9, 5);
        }
        assert!(w.progress(0.0).is_none());
        // Gleiches Item in beiden Haenden ist kein Tausch.
        let mut w = SwapWatch::default();
        w.tick(5, 5);
        w.tick(5, 5);
        assert!(w.progress(0.0).is_none());
    }

    #[test]
    fn laternen_und_schwerter() {
        assert!(held_up("soul_lantern") && !held_up("jack_o_lantern") && !held_up("sea_lantern"));
        assert_eq!(lantern_block_model("waxed_oxidized_copper_lantern").as_deref(), Some("block/oxidized_copper_lantern"));
        assert!(is_sword("netherite_sword") && !is_sword("sword_fish"));
    }

    // Eine ruhende Laterne haengt gerade; bewegt sich die Hand, schwingt sie nach.
    #[test]
    fn laterne_pendelt_nach() {
        let mut l = LanternSwing::default();
        for t in 0..40 {
            l.update([0.0, 64.0, 0.0], t, 0.0, 0.0, 0.0);
        }
        let (x, z) = l.update([0.0, 64.0, 0.0], 40, 0.0, 0.0, 0.0);
        assert!(x.abs() < 1e-3 && z.abs() < 1e-3);
        let mut bewegt = false;
        for t in 41..60 {
            let (x, z) = l.update([0.0, 64.0, (t - 40) as f64 * 0.3], t, 0.0, 0.0, 0.0);
            bewegt |= x.abs() > 1.0 || z.abs() > 1.0;
        }
        assert!(bewegt);
    }

    #[test]
    fn glaettung_holt_je_tick_neun_zehntel_auf() {
        let mut g = Smoothing::default();
        let mut pose = basis();
        g.apply(&mut pose, 0.0, [false; 4]);
        let mut ziel = basis();
        ziel.rotate("right_arm", [-1.0, 0.0, 0.0]);
        g.apply(&mut ziel, 0.05, [false; 4]);
        assert!((ziel.get("right_arm").rotation[0] + 0.9).abs() < 1e-4);
        let mut sofort = basis();
        sofort.rotate("right_arm", [1.0, 0.0, 0.0]);
        g.apply(&mut sofort, 0.05, [true, false, false, false]);
        assert_eq!(sofort.get("right_arm").rotation[0], 1.0);
    }
}
