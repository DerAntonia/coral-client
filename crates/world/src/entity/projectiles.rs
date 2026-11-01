// Geschosse: Arten, Bewegung mit Kollision, Waffen und Trefferpruefung.

use super::*;

// Klassischer Slab-Test: pro Achse das Parameterintervall [t_min, t_max] schneiden, in
// dem der Strahl zwischen den beiden Ebenen liegt. Bleibt ein nichtleeres Intervall mit
// t_max >= 0 uebrig, trifft der Strahl; Eintritt bei max(t_min, 0) (Start IN der Box
// zaehlt als Treffer bei 0). `dir` muss nicht normiert sein -- max_dist ist dann in
// Einheiten der dir-Laenge; der Aufrufer uebergibt einen Einheitsvektor.
// --- Projektile -----------------------------------------------------------------------
//
// Pfeile und Wurfgeschosse fliegen in Vanilla auf dem CLIENT. Der Server schickt beim Spawn
// die Anfangsgeschwindigkeit und danach nur noch grobe Korrekturen; dazwischen rechnet jeder
// Client dieselbe Bahn. Deshalb sehen Pfeile fluessig aus, obwohl kaum Pakete kommen.
//
// Fuer fremde Entities interpolieren wir sonst nur zwischen Server-Positionen -- bei einem
// Pfeil ist das sichtbar falsch: er ruckelt, haengt hinterher und landet optisch woanders.
//
// WICHTIG ist die REIHENFOLGE, und sie ist fuer die beiden Familien VERSCHIEDEN
// (1.21.11 nachgesehen):
//
//   ThrowableProjectile.tick():  applyGravity() -> applyInertia() -> bewegen
//   AbstractArrow.tick():        (Wasserreibung) -> bewegen -> applyInertia(0.99) -> applyGravity()
//
// Wer beides gleich behandelt, bekommt eine Bahn, die pro Tick um genau einen
// Schwerkraftschritt danebenliegt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectileKind {
    // AbstractArrow.getDefaultGravity() = 0.05, Reibung 0.99, im Wasser getWaterInertia() = 0.6.
    Arrow,
    // ThrowableProjectile.getDefaultGravity() = 0.03, Reibung 0.99, im Wasser 0.8.
    Throwable,
    // AbstractWindCharge (Windkugel, auch die der Breeze): AbstractHurtingProjectile ohne
    // Beschleunigung (accelerationPower = 0) und mit Traegheit 1.0 in Luft wie Wasser -- sie
    // fliegt also schnurgerade mit ihrer Startgeschwindigkeit weiter. Der Server schickt ihre
    // Position nur alle zehn Ticks (updateInterval 10); ohne eigenen Flug stand sie dazwischen
    // still und sprang dann.
    WindCharge,
}

impl ProjectileKind {
    pub(crate) const fn gravity(self) -> f64 {
        match self {
            Self::Arrow => 0.05,
            Self::Throwable => 0.03,
            Self::WindCharge => 0.0,
        }
    }

    pub(crate) const fn water_inertia(self) -> f64 {
        match self {
            Self::Arrow => 0.6,
            Self::Throwable => 0.8,
            Self::WindCharge => 1.0,
        }
    }
}

// Block.getSlipperiness: 0.6, Eis 0.98, Blaueis 0.989, Schleim 0.8.
pub(crate) fn block_slipperiness(state: u32) -> f64 {
    match crate::block_name(state).unwrap_or("") {
        "ice" | "packed_ice" | "frosted_ice" => 0.98,
        "blue_ice" => 0.989,
        "slime_block" => 0.8,
        _ => 0.6,
    }
}

// Entity.move fuer eine fusszentrierte Box (halbe Breite `half`, Hoehe `height`) gegen die
// Kollisionsformen der Bloecke: erst Y, dann die groessere der beiden waagerechten Achsen
// (Entity.adjustMovementForCollisions). Rueckgabe: neue Position und je Achse, ob sie
// angestossen ist.
pub fn move_with_collisions(
    pos: (f64, f64, f64),
    vel: (f64, f64, f64),
    half: f64,
    height: f64,
    world: &crate::chunk::World,
    shapes: &crate::BlockShapes,
) -> ((f64, f64, f64), [bool; 3]) {
    let mut min = [pos.0 - half, pos.1, pos.2 - half];
    let mut max = [pos.0 + half, pos.1 + height, pos.2 + half];
    let bewegung = [vel.0, vel.1, vel.2];
    // Alle Kollisionsboxen im ueberstrichenen Bereich (plus ein Block Rand fuer hohe Formen
    // wie Zaeune).
    let mut boxen: Vec<([f64; 3], [f64; 3])> = Vec::new();
    let lo = [0, 1, 2].map(|a| (min[a] + bewegung[a].min(0.0)).floor() as i32 - 1);
    let hi = [0, 1, 2].map(|a| (max[a] + bewegung[a].max(0.0)).floor() as i32 + 1);
    if (0..3).any(|a| hi[a] - lo[a] > 16) {
        // Unplausibel schnell -- lieber ohne Kollision als tausende Bloecke abfragen.
        return ((pos.0 + vel.0, pos.1 + vel.1, pos.2 + vel.2), [false; 3]);
    }
    for bx in lo[0]..=hi[0] {
        for by in lo[1]..=hi[1] {
            for bz in lo[2]..=hi[2] {
                let state = world.block_at(bx, by, bz);
                if state == crate::STATE_AIR {
                    continue;
                }
                for b in crate::collision_boxes(state, shapes) {
                    let o = [f64::from(bx), f64::from(by), f64::from(bz)];
                    boxen.push(([0, 1, 2].map(|i| o[i] + f64::from(b.min[i])), [0, 1, 2].map(|i| o[i] + f64::from(b.max[i]))));
                }
            }
        }
    }
    let reihenfolge = if bewegung[0].abs() < bewegung[2].abs() { [1, 2, 0] } else { [1, 0, 2] };
    let mut stoss = [false; 3];
    for achse in reihenfolge {
        let mut d = bewegung[achse];
        if d == 0.0 {
            continue;
        }
        let (a1, a2) = match achse {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        for (bmin, bmax) in &boxen {
            // Nur Boxen, die sich auf den beiden anderen Achsen mit uns ueberlappen.
            if bmax[a1] <= min[a1] + 1.0e-7 || bmin[a1] >= max[a1] - 1.0e-7 || bmax[a2] <= min[a2] + 1.0e-7 || bmin[a2] >= max[a2] - 1.0e-7 {
                continue;
            }
            if d > 0.0 && bmin[achse] >= max[achse] - 1.0e-7 {
                d = d.min(bmin[achse] - max[achse]);
            } else if d < 0.0 && bmax[achse] <= min[achse] + 1.0e-7 {
                d = d.max(bmax[achse] - min[achse]);
            }
        }
        if d != bewegung[achse] {
            stoss[achse] = true;
        }
        min[achse] += d;
        max[achse] += d;
    }
    ((min[0] + half, min[1], min[2] + half), stoss)
}

// Welche Entity-Typen der Client selbst fliegen laesst.
//
// BEWUSST NICHT dabei sind die Feuerkugeln (fireball, small_fireball, wither_skull,
// dragon_fireball): die haben eine ganz andere Bahn (keine Schwerkraft, dafuer eine
// Beschleunigung entlang der Anfangsrichtung) und gehoeren erst dazu, wenn ihre Physik
// genauso belegt ist wie die hier.
#[must_use] 
pub fn projectile_kind(entity_type: i32) -> Option<ProjectileKind> {
    match entity_type_name(entity_type)? {
        "arrow" | "spectral_arrow" | "trident" => Some(ProjectileKind::Arrow),
        "ender_pearl" | "snowball" | "egg" | "experience_bottle" | "splash_potion" | "lingering_potion" => {
            Some(ProjectileKind::Throwable)
        }
        "wind_charge" | "breeze_wind_charge" => Some(ProjectileKind::WindCharge),
        _ => None,
    }
}

// Ueber so viele Ticks laeuft der optische Abschuss-Versatz wieder auf Null.
// Drei Ticks sind rund 150 ms; ein Pfeil hat in der Zeit schon rund neun Bloecke zurueck-
// gelegt, der Versatz von gut einem Drittel Block faellt dann nicht mehr auf.
pub const LAUNCH_BLEND_TICKS: u32 = 3;

// Womit geschossen wurde. Bestimmt, WO die Hand sitzt: die Armbrust liegt beim Schuss
// deutlich tiefer als der gespannte Bogen, und ein geworfener Gegenstand wieder anders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchWeapon {
    Bow,
    Crossbow,
    Hand,
}

// Handposition relativ zum Spawnpunkt des Geschosses, in der Reihenfolge
// (rechts, hoch, vorwaerts) und in Bloecken.
//
// Diese Zahlen stehen in KEINER Vanilla-Klasse -- Vanilla hat den Versatz gar nicht, es gibt
// also nichts nachzuschlagen. Sie sind nach dem Spielermodell abgeschaetzt: der Armdrehpunkt
// liegt bei 5/16 Block neben der Koerperachse, der ausgestreckte Arm reicht rund einen
// Drittelblock nach vorn, und der Spawnpunkt liegt auf Augenhoehe minus 0.1. Wer sie
// nachjustieren will, aendert sie hier und nirgends sonst.
impl LaunchWeapon {
    pub(crate) const fn hand(self) -> [f32; 3] {
        match self {
            // Gespannter Bogen: die Hand ist fast auf Augenhoehe, weit vorn.
            Self::Bow => [0.28, -0.10, 0.36],
            // Armbrust: im Anschlag vor der Brust, also deutlich tiefer.
            Self::Crossbow => [0.30, -0.32, 0.34],
            // Geworfen (Perle, Schneeball, Ei): Wurf aus der Schulter, dazwischen.
            Self::Hand => [0.32, -0.22, 0.30],
        }
    }
}

// Was in dieser Hand steckt -- None, wenn es kein Fernkampfgegenstand ist.
pub(crate) fn weapon_of(item: Option<&HeldItem>) -> Option<LaunchWeapon> {
    match crate::item_registry::item_name(item?.item_id)? {
        "bow" => Some(LaunchWeapon::Bow),
        "crossbow" => Some(LaunchWeapon::Crossbow),
        _ => None,
    }
}

// Aus welcher Hand kam der Schuss und womit.
//
// Der Bogen bleibt nach dem Schuss in der Hand, also reicht ein Blick auf die Ausruestung --
// und der ist verlaesslicher als das Benutzungs-Bit in den Metadaten, das der Server im
// selben Tick schon wieder loescht. Liegt in beiden Haenden eine Waffe, gewinnt die
// Haupthand, wie in Vanilla auch (Player.getItemInHand wird von der Haupthand aus geprueft).
// Ohne Bogen oder Armbrust wurde geworfen.
#[must_use] 
pub fn shooting_hand(main_hand: Option<&HeldItem>, off_hand: Option<&HeldItem>) -> (bool, LaunchWeapon) {
    if let Some(w) = weapon_of(main_hand) {
        return (false, w);
    }
    if let Some(w) = weapon_of(off_hand) {
        return (true, w);
    }
    (false, LaunchWeapon::Hand)
}

// Der optische Versatz in WELTkoordinaten: die Handposition des Schuetzen, gedreht in seine
// Blickrichtung.
//
// Basis wie in Vanilla: Blickrichtung ist (-sin y * cos p, -sin p, cos y * cos p), der
// Aufwaertsvektor dieselbe Formel mit p - 90 Grad, und "rechts" steht bei Gierwinkel 0
// (Blick nach +Z) auf -X. `offhand` spiegelt nur die Seite -- deshalb wandert der Versatz
// mit, wenn man den Bogen mit F in die andere Hand legt.
#[must_use] 
pub fn launch_hand_offset(yaw_deg: f32, pitch_deg: f32, offhand: bool, weapon: LaunchWeapon) -> (f32, f32, f32) {
    let [rechts_anteil, hoch_anteil, vor_anteil] = weapon.hand();
    let rechts_anteil = if offhand { -rechts_anteil } else { rechts_anteil };
    let (y, p) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    let (sy, cy) = (y.sin(), y.cos());
    let (sp, cp) = (p.sin(), p.cos());
    let vor = [-sy * cp, -sp, cy * cp];
    let hoch = [-sy * sp, cp, cy * sp];
    let rechts = [-cy, 0.0, -sy];
    let achse = |i: usize| rechts[i] * rechts_anteil + hoch[i] * hoch_anteil + vor[i] * vor_anteil;
    (achse(0), achse(1), achse(2))
}

// Projectile.lerpRotation: ein 20-%-Schritt auf den Zielwinkel, ueber die 360-Grad-Naht hinweg.
pub(crate) fn lerp_rotation(current: f32, target: f32) -> f32 {
    let mut c = current;
    while target - c < -180.0 {
        c -= 360.0;
    }
    while target - c >= 180.0 {
        c += 360.0;
    }
    c + (target - c) * 0.2
}

// Der erste Blocktreffer auf der Strecke origin -> origin+delta, als Anteil 0..1 von delta.
// None = freie Bahn. Gemessen wird gegen die KOLLISIONSform (Vanillas Block.COLLIDER), nicht
// gegen den Umriss: ein Pfeil fliegt durch hohes Gras und bleibt an einer Stufe haengen.
// Liegt der Punkt in der Kollisionsform des Blocks, in dem er steht (AABB.contains)?
pub(crate) fn point_in_collision(p: (f64, f64, f64), world: &crate::chunk::World, shapes: &crate::BlockShapes) -> bool {
    let (bx, by, bz) = (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
    let state = world.block_at(bx, by, bz);
    if state == crate::chunk::STATE_AIR {
        return false;
    }
    let (lx, ly, lz) = (p.0 - f64::from(bx), p.1 - f64::from(by), p.2 - f64::from(bz));
    crate::collision::collision_boxes(state, shapes).iter().any(|b| {
        lx >= f64::from(b.min[0]) && lx < f64::from(b.max[0]) && ly >= f64::from(b.min[1]) && ly < f64::from(b.max[1]) && lz >= f64::from(b.min[2]) && lz < f64::from(b.max[2])
    })
}

pub(crate) fn block_hit_along(
    origin: (f64, f64, f64),
    delta: (f64, f64, f64),
    world: &crate::chunk::World,
    shapes: &crate::BlockShapes,
) -> Option<f64> {
    let ziel = (origin.0 + delta.0, origin.1 + delta.1, origin.2 + delta.2);
    let min = (origin.0.min(ziel.0), origin.1.min(ziel.1), origin.2.min(ziel.2));
    let max = (origin.0.max(ziel.0), origin.1.max(ziel.1), origin.2.max(ziel.2));
    let mut naechster: Option<f64> = None;
    for bx in min.0.floor() as i32..=max.0.floor() as i32 {
        for by in min.1.floor() as i32..=max.1.floor() as i32 {
            for bz in min.2.floor() as i32..=max.2.floor() as i32 {
                let state = world.block_at(bx, by, bz);
                if state == crate::chunk::STATE_AIR {
                    continue;
                }
                for b in crate::collision::collision_boxes(state, shapes) {
                    let kasten_min = (f64::from(bx) + f64::from(b.min[0]), f64::from(by) + f64::from(b.min[1]), f64::from(bz) + f64::from(b.min[2]));
                    let kasten_max = (f64::from(bx) + f64::from(b.max[0]), f64::from(by) + f64::from(b.max[1]), f64::from(bz) + f64::from(b.max[2]));
                    if let Some(t) = ray_aabb(origin, delta, kasten_min, kasten_max) {
                        if (0.0..=1.0).contains(&t) && naechster.is_none_or(|bisher| t < bisher) {
                            naechster = Some(t);
                        }
                    }
                }
            }
        }
    }
    naechster
}

pub(crate) fn ray_aabb(origin: (f64, f64, f64), dir: (f64, f64, f64), min: (f64, f64, f64), max: (f64, f64, f64)) -> Option<f64> {
    let mut t_min = f64::NEG_INFINITY;
    let mut t_max = f64::INFINITY;
    for ((o, d), (lo, hi)) in
        [((origin.0, dir.0), (min.0, max.0)), ((origin.1, dir.1), (min.1, max.1)), ((origin.2, dir.2), (min.2, max.2))]
    {
        if d.abs() < 1e-12 {
            // Strahl parallel zu diesen Ebenen: trifft nur, wenn er dazwischen startet.
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((lo - o) / d, (hi - o) / d);
        let (t0, t1) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
        t_min = t_min.max(t0);
        t_max = t_max.min(t1);
        if t_min > t_max {
            return None;
        }
    }
    (t_max >= 0.0).then_some(t_min.max(0.0))
}
