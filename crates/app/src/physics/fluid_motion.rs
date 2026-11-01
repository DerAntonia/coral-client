// Fluessigkeiten, Gleiten, Feuerwerk, Posen und Schleichen an Kanten.

use super::*;

// updateFluidHeightAndDoFluidPushing() ohne den Stroemungs-Schub-Vektor (s. Modul-Kommentar):
// AABB um 0.001 nach innen verkleinert, jede ueberlappte Fluessigkeits-Zelle derselben Art
// gegen die Boxunterkante geprueft, hoechste Ueberschneidung gewinnt. None = nicht beruehrt,
// Some(h) = Fuellstand in Blockeinheiten oberhalb der Boxunterkante (kann 0.0 sein: gerade so
// beruehrt).
// Ergebnis eines Fluessigkeits-Scans ueber die Spielerbox.
pub(crate) struct FluidScan {
    // Wie weit die Fluessigkeitsoberflaeche ueber der Boxunterkante steht (Vanillas `d`).
    pub(crate) height: f64,
    // Aufsummierter, noch UNgewichteter Stroemungsvektor aller beruehrten Zellen, plus deren
    // Anzahl -- die Mittelung passiert erst beim Anwenden (s. apply_fluid_push).
    pub(crate) push: DVec3,
    pub(crate) push_cells: u32,
}

pub(crate) fn fluid_height_scan(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes, kind: world::FluidKind) -> Option<FluidScan> {
    let min = min + DVec3::splat(0.001);
    let max = max - DVec3::splat(0.001);
    if min.x >= max.x || min.y >= max.y || min.z >= max.z {
        return None;
    }

    let (bx0, bx1) = (min.x.floor() as i32, max.x.ceil() as i32);
    let (by0, by1) = (min.y.floor() as i32, max.y.ceil() as i32);
    let (bz0, bz1) = (min.z.floor() as i32, max.z.ceil() as i32);

    let mut touching = false;
    let mut e = 0.0f64;
    let mut push = DVec3::ZERO;
    let mut push_cells = 0u32;
    for bx in bx0..bx1 {
        for by in by0..by1 {
            for bz in bz0..bz1 {
                let state = world.block_at(bx, by, bz);
                if world::fluid_kind(state) != Some(kind) {
                    continue;
                }
                let Some(h) = world::fluid_height_at(world, bx, by, bz) else {
                    continue;
                };
                let f = f64::from(by) + h;
                if f >= min.y {
                    touching = true;
                    e = e.max(f - min.y);
                    // Stroemungsschub, exakt wie Entity.updateMovementInFluid: der Vektor
                    // JEDER beruehrten Zelle zaehlt einzeln, flach eintauchende Zellen
                    // (e < 0.4) nur anteilig. `e` ist hier bewusst das LAUFENDE Maximum und
                    // nicht das Endergebnis -- Vanilla skaliert ebenfalls mit dem Stand
                    // waehrend der Schleife, das Ergebnis haengt damit an der
                    // Iterationsreihenfolge. Nachgebaut statt "verbessert", weil der Server
                    // (Anticheat) genau diese Reihenfolge rechnet.
                    if let Some(v) = world::fluid_flow_velocity(world, shapes, bx, by, bz) {
                        let mut v = DVec3::new(v[0], v[1], v[2]);
                        if e < 0.4 {
                            v *= e;
                        }
                        push += v;
                        push_cells += 1;
                    }
                }
            }
        }
    }
    touching.then_some(FluidScan { height: e, push, push_cells })
}

// Wendet den in `scan` gesammelten Stroemungsschub auf die Geschwindigkeit an -- zweite
// Haelfte von Entity.updateMovementInFluid.
//
// Zwei Feinheiten, die man leicht uebersieht und die beide fuer Anticheat-Gleichlauf zaehlen:
//   * Der gemittelte Vektor wird fuer SPIELER NICHT normiert (Vanilla normiert nur fuer
//     Nicht-Spieler). Ein Spieler in schwacher Stroemung wird also
//     wirklich schwaecher geschoben, ein Mob nicht.
//   * Steht man praktisch still und waere der Schub extrem klein, wird er auf eine
//     Mindestlaenge hochgezogen -- sonst bliebe man in ganz flachem Wasser ewig kleben.
pub(crate) fn apply_fluid_push(velocity: &mut DVec3, scan: &FluidScan, speed: f64) {
    if scan.push_cells == 0 || scan.push.length() <= 0.0 {
        return;
    }
    let mut v = scan.push / f64::from(scan.push_cells);
    v *= speed;
    if velocity.x.abs() < FLUID_PUSH_STILL_EPS && velocity.z.abs() < FLUID_PUSH_STILL_EPS && v.length() < FLUID_PUSH_MIN_SPEED {
        let len = v.length();
        if len > 0.0 {
            v = v / len * FLUID_PUSH_MIN_SPEED;
        }
    }
    *velocity += v;
}

// WorldView.containsFluid(box): beruehrt die Box IRGENDEINE Zelle mit nicht-leerem
// FluidState? Bewusst etwas ganz anderes als fluid_height_scan darueber:
//   * kein 0.001-Schrumpfen der Box,
//   * KEINE Fuellstands-Bedingung -- schon eine Zelle mit level=7 (Fuellhoehe ~0.1) zaehlt,
//     auch wenn ihre Oberflaeche weit unter der Boxunterkante liegt,
//   * Zellbereich floor(min)..ceil(max) mit AUSSCHLIESSENDER Obergrenze.
// Genau diese Unterschiede waren der Grund, warum der Wasser-Hop bisher zu oft feuerte
// (s. jump_out_of_fluid). fluid_kind() deckt Quell-/Fliessbloecke, waterlogged Bloecke und
// die immer-nassen (Seegras & Co.) ab -- also dasselbe wie !getFluidState().isEmpty().
pub(crate) fn contains_fluid(min: DVec3, max: DVec3, world: &World) -> bool {
    let (x0, x1) = (min.x.floor() as i32, max.x.ceil() as i32);
    let (y0, y1) = (min.y.floor() as i32, max.y.ceil() as i32);
    let (z0, z1) = (min.z.floor() as i32, max.z.ceil() as i32);
    for bx in x0..x1 {
        for by in y0..y1 {
            for bz in z0..z1 {
                if world::fluid_kind(world.block_at(bx, by, bz)).is_some() {
                    return true;
                }
            }
        }
    }
    false
}

// getFluidFallingAdjustedMovement(): Gravitation im Wasser/Lava NICHT voll, sondern nur
// d/16 pro Tick abziehen (und beim Sprinten GAR NICHT -- Sprint-Schwimmen bleibt auf Hoehe),
// plus ein Sonderfall, der beim Wiedereintauchen einen winzigen konstanten Sinkwert
// (-0.003) statt der Formel nimmt, um Ruckeln an der Wasseroberflaeche zu vermeiden.
// LivingEntity.getEffectiveGravity: mit Sanftem Fall faellt man hoechstens mit 0.01 --
// aber nur FALLEND (vel.y <= 0). Nach oben (Sprung) wirkt die volle Schwerkraft.
pub(crate) fn effective_gravity(vel_y: f64, input: &Input, gravity: f64) -> f64 {
    if vel_y <= 0.0 && input.slow_falling {
        gravity.min(0.01)
    } else {
        gravity
    }
}

// Block-Tag can_glide_through (Datenpaket des Jars), aufgeloest: gleitend kein Klettern.
pub(crate) const CAN_GLIDE_THROUGH: [&str; 7] =
    ["vine", "twisting_vines", "twisting_vines_plant", "weeping_vines", "weeping_vines_plant", "cave_vines", "cave_vines_plant"];

// Entity.getRotationVector(pitch, yaw): alles in float und mit der Sinus-Tabelle, erst das
// Ergebnis wird double. `pitch_deg` positiv = nach unten (Vanillas Konvention).
pub fn rotation_vector(pitch_deg: f32, yaw_deg: f32) -> DVec3 {
    const RADIANS_PER_DEGREE: f32 = (std::f64::consts::PI / 180.0) as f32;
    let f = pitch_deg * RADIANS_PER_DEGREE;
    let g = -yaw_deg * RADIANS_PER_DEGREE;
    let h = mc_cos(f64::from(g));
    let i = mc_sin(f64::from(g));
    let j = mc_cos(f64::from(f));
    let k = mc_sin(f64::from(f));
    DVec3::new(f64::from(i * j), f64::from(-k), f64::from(h * j))
}

// LivingEntity.calcGlidingVelocity, in eigenen Worten:
//   - Schwerkraft, abgeschwaecht je flacher man blickt (cos^2 des Pitch, NICHT aus der
//     Tabelle, sondern Math.cos): waagerecht wirkt nur ein Viertel.
//   - Sinkt man, wird ein Teil des Sinkens in Vorwaertsfahrt umgesetzt.
//   - Blickt man nach OBEN, wird Fahrt in Hoehe umgesetzt (Tabellen-Sinus).
//   - Die waagerechte Geschwindigkeit dreht sich zu 10 % pro Tick in die Blickrichtung.
//   - Zum Schluss Daempfung 0.99 / 0.98 / 0.99 (float-Konstanten).
pub(crate) fn calc_gliding_velocity(alt: DVec3, rot: DVec3, pitch_deg: f32, gravity: f64) -> DVec3 {
    const RADIANS_PER_DEGREE: f32 = (std::f64::consts::PI / 180.0) as f32;
    let f = pitch_deg * RADIANS_PER_DEGREE;
    let d = (rot.x * rot.x + rot.z * rot.z).sqrt();
    let e = (alt.x * alt.x + alt.z * alt.z).sqrt();
    let h = f64::from(f).cos().powi(2);
    let mut v = alt + DVec3::new(0.0, gravity * (-1.0 + h * 0.75), 0.0);
    if v.y < 0.0 && d > 0.0 {
        let i = v.y * -0.1 * h;
        v += DVec3::new(rot.x * i / d, i, rot.z * i / d);
    }
    if f < 0.0 && d > 0.0 {
        let i = e * f64::from(-mc_sin(f64::from(f))) * 0.04;
        v += DVec3::new(-rot.x * i / d, i * 3.2, -rot.z * i / d);
    }
    if d > 0.0 {
        v.x += (rot.x / d * e - v.x) * 0.1;
        v.z += (rot.z / d * e - v.z) * 0.1;
    }
    DVec3::new(v.x * f64::from(0.99f32), v.y * f64::from(0.98f32), v.z * f64::from(0.99f32))
}

// FireworkRocketEntity.tick, der Teil fuer eine Rakete am gleitenden Schuetzen: die
// Geschwindigkeit zieht zur Haelfte auf 1.5 Bloecke/Tick in Blickrichtung, dazu 0.1 Schub.
pub fn firework_boost(vel: DVec3, rot: DVec3) -> DVec3 {
    vel + DVec3::new(
        rot.x * 0.1 + (rot.x * 1.5 - vel.x) * 0.5,
        rot.y * 0.1 + (rot.y * 1.5 - vel.y) * 0.5,
        rot.z * 0.1 + (rot.z * 1.5 - vel.z) * 0.5,
    )
}

pub(crate) fn fluid_falling_adjusted_movement(vel: DVec3, gravity: f64, was_falling: bool, sprinting: bool) -> DVec3 {
    if gravity == 0.0 || sprinting {
        return vel;
    }
    let e = if was_falling && (vel.y - 0.005).abs() >= 0.003 && (vel.y - gravity / 16.0).abs() < 0.003 {
        -0.003
    } else {
        vel.y - gravity / 16.0
    };
    DVec3::new(vel.x, e, vel.z)
}

// canChangeIntoPose(pose): wuerde eine Box der gegebenen Hoehe an der aktuellen Position
// IRGENDETWAS Solides ueberlappen? Anders als sweep_axis (das eine BEWEGUNG um ein Delta
// klemmt) hier ein reiner STATISCHER Ueberlappungstest -- sweep_axis selbst kann das nicht
// leisten, weil es bei delta=0 sofort abbricht (s. dortigen fruehen Return). Dieselbe
// Block-Scan-/Kollisionsbox-Logik wie sweep_axis, nur ohne Bewegungsrichtung.
pub(crate) fn pose_fits(pos: DVec3, height: f64, world: &World, shapes: &world::BlockShapes, ctx: world::ShapeContext, body: BodyAttributes) -> bool {
    let half = body.width() / 2.0;
    // Vanilla verkleinert die Posen-Box vor der Pruefung um 1e-7 je Seite
    // (canChangeIntoPose: `...getBoxAt(pos).contract(1.0E-7)`, und contract(v) ist
    // expand(-v) auf allen sechs Seiten). GrimAC macht exakt dasselbe (canEnterPose:
    // `getBoundingBoxForPose(...).expand(-1.0E-7)`). Ein Block, der die Box nur genau
    // beruehrt, zaehlt damit NICHT als Kollision -- ohne diese Verkleinerung haengt das
    // Ergebnis an der letzten Nachkommastelle der Spielerposition, und genau dort liefen
    // wir und Grim auseinander.
    const POSE_SHRINK: f64 = 1.0e-7;
    box_is_empty(
        DVec3::new(pos.x - half + POSE_SHRINK, pos.y + POSE_SHRINK, pos.z - half + POSE_SHRINK),
        DVec3::new(pos.x + half - POSE_SHRINK, pos.y + height - POSE_SHRINK, pos.z + half - POSE_SHRINK),
        world,
        shapes,
        ctx,
    )
}

// true, wenn die AABB [min,max] keinerlei Block-Kollisionsbox ueberlappt (World.isSpaceEmpty).
// Aus pose_fits herausgezogen, damit auch der Sneak-Kantenschutz denselben Ueberlappungstest
// nutzt (s. adjust_for_sneaking).
pub(crate) fn box_is_empty(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes, ctx: world::ShapeContext) -> bool {
    let bx0 = min.x.floor() as i32;
    let bx1 = (max.x - EPS).floor() as i32;
    let by0 = min.y.floor() as i32;
    let by1 = (max.y - EPS).floor() as i32;
    let bz0 = min.z.floor() as i32;
    let bz1 = (max.z - EPS).floor() as i32;

    for bx in bx0..=bx1 {
        for by in by0..=by1 {
            for bz in bz0..=bz1 {
                let state = world.block_at(bx, by, bz);
                let (odx, odz) = world::block_name(state)
                    .map_or((0.0, 0.0), |name| world::block_xz_offset(name, bx, bz));
                for cbox in world::collision_boxes_at(state, shapes, by, ctx) {
                    let bmin = DVec3::new(
                        f64::from(bx) + f64::from(odx) + f64::from(cbox.min[0]),
                        f64::from(by) + f64::from(cbox.min[1]),
                        f64::from(bz) + f64::from(odz) + f64::from(cbox.min[2]),
                    );
                    let bmax = DVec3::new(
                        f64::from(bx) + f64::from(odx) + f64::from(cbox.max[0]),
                        f64::from(by) + f64::from(cbox.max[1]),
                        f64::from(bz) + f64::from(odz) + f64::from(cbox.max[2]),
                    );
                    let overlaps = min.x < bmax.x - EPS
                        && max.x > bmin.x + EPS
                        && min.y < bmax.y - EPS
                        && max.y > bmin.y + EPS
                        && min.z < bmax.z - EPS
                        && max.z > bmin.z + EPS;
                    if overlaps {
                        return false;
                    }
                }
            }
        }
    }
    true
}

// Sneak-Kantenschutz -- PlayerEntity.adjustMovementForSneaking(), byte-getreu aus dem
// Yarn-1.21.11-Jar. Beim Schleichen (clipAtLedge() == isSneaking()) auf einer Flaeche
// (isStandingOnSurface, hier ueber on_ground) und bei Abwaerts-/Horizontalbewegung
// (movement.y <= 0) werden X und Z in 0.05-Schritten reduziert, solange der Boden-Streifen
// unter der um (dx,dz) verschobenen Box LEER waere -- man clippt also an der Blockkante
// statt hinunterzufallen. Kein Anticheat-Belang (Grim flaggt das Fehlen nicht), reines
// Vanilla-Spielgefuehl.
// UseEffectsComponent.DEFAULT.speedMultiplier() -- der Faktor, mit dem Vanilla die
// Bewegungseingabe waehrend einer Item-Benutzung daempft (wirksam: Input::item_use_speed).
#[cfg(test)]
pub const ITEM_USE_SPEED_MULT: f64 = 0.2;
pub(crate) const SNEAK_STEP: f64 = 0.05; // STEPPING_APPROACH_DISTANCE

// MathHelper.approximatelyEquals -- Vanillas Vergleich fuer horizontalCollision.
pub(crate) fn approximately_equals(a: f64, b: f64) -> bool {
    (b - a).abs() < 1.0e-5
}

// ClientPlayerEntity.isHorizontalCollisionMinor(movement), nach dem 1.21.11-Client: die
// gewollte Richtung entsteht aus den Eingabe-Impulsen, um den Yaw gedreht (Vanillas sin/cos).
// Ist ihr Laengenquadrat oder das der tatsaechlichen waagerechten Bewegung unter 1e-5, ist der
// Kontakt nicht "minor". Sonst entscheidet der Winkel zwischen beiden (acos des normierten
// Skalarprodukts) gegen 0.13962634 rad.
//
// Gelesen: zeigt die tatsaechlich zurueckgelegte Bewegung noch innerhalb von 8 Grad
// (0.13962634 rad) in die gewollte Richtung, war der Wandkontakt nur ein Streifen.
//
// `sidewaysSpeed`/`forwardSpeed` sind die ROHEN Eingabe-Impulse (-1/0/1), nicht die
// normierten aus add_move_input -- fuer den Winkel ist die Laenge ohnehin egal, sie geht nur
// in die beiden 1e-5-Schwellen ein, und dort sind die Rohwerte das Richtige.
// sin/cos kommen aus Vanillas Float-Tabelle (mc_sin/mc_cos), damit der Winkel bitgleich faellt.
pub(crate) fn is_horizontal_collision_minor(input: &Input, achieved: DVec3) -> bool {
    let (sin_y, cos_y) = (f64::from(mc_sin(f64::from(input.yaw_rad))), f64::from(mc_cos(f64::from(input.yaw_rad))));
    let (strafe, forward) = (f64::from(input.strafe), f64::from(input.forward));
    let g = strafe * cos_y - forward * sin_y;
    let h = forward * cos_y + strafe * sin_y;
    let i = g * g + h * h;
    let j = achieved.x * achieved.x + achieved.z * achieved.z;
    // Die Schwellen sind in Vanilla float-Literale, der Vergleich laeuft aber in double.
    if i < f64::from(1.0e-5f32) || j < f64::from(1.0e-5f32) {
        return false;
    }
    let k = g * achieved.x + h * achieved.z;
    (k / (i * j).sqrt()).acos() < f64::from(0.13962634f32)
}

// PlayerEntity.isSpaceAroundPlayerEmpty(offsetX, offsetZ, stepHeight): der Streifen
// minY-stepHeight .. minY unter der um (dx,dz) verschobenen Fussflaeche, ringsum um 1e-7 nach
// innen geschrumpft. LEER heisst: dort unten ist kein Block -- also eine Kante.
// Die Schritthoehe ist ein PARAMETER, kein fester Wert: isStandingOnSurface reicht
// stepHeight - fallDistance herein.
pub(crate) fn space_around_player_empty(
    pos: DVec3,
    dx: f64,
    dz: f64,
    step_height: f64,
    world: &World,
    shapes: &world::BlockShapes,
    ctx: world::ShapeContext,
    body: BodyAttributes,
) -> bool {
    let half = body.width() / 2.0;
    let e = 1.0e-7;
    let min = DVec3::new(pos.x - half + e + dx, pos.y - step_height - e, pos.z - half + e + dz);
    let max = DVec3::new(pos.x + half - e + dx, pos.y, pos.z + half - e + dz);
    box_is_empty(min, max, world, shapes, ctx)
}

pub(crate) fn adjust_for_sneaking(
    pos: DVec3,
    vel_x: f64,
    vel_z: f64,
    world: &World,
    shapes: &world::BlockShapes,
    ctx: world::ShapeContext,
    body: BodyAttributes,
) -> (f64, f64) {
    let space_empty = |dx: f64, dz: f64| -> bool { space_around_player_empty(pos, dx, dz, body.step_height, world, shapes, ctx, body) };
    let mut d = vel_x;
    let mut e = vel_z;
    let hx = d.signum() * SNEAK_STEP;
    let hz = e.signum() * SNEAK_STEP;
    // Erst X allein, dann Z allein, dann beide gemeinsam abbauen -- exakt Vanillas drei Schleifen.
    while d != 0.0 && space_empty(d, 0.0) {
        if d.abs() <= SNEAK_STEP {
            d = 0.0;
            break;
        }
        d -= hx;
    }
    while e != 0.0 && space_empty(0.0, e) {
        if e.abs() <= SNEAK_STEP {
            e = 0.0;
            break;
        }
        e -= hz;
    }
    while d != 0.0 && e != 0.0 && space_empty(d, e) {
        if d.abs() <= SNEAK_STEP {
            d = 0.0;
        } else {
            d -= hx;
        }
        if e.abs() <= SNEAK_STEP {
            e = 0.0;
        } else {
            e -= hz;
        }
    }
    (d, e)
}
