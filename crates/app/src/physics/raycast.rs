// Block-Raycasts, Zuschauer-Geschwindigkeit und Bewegen mit Kollision.

use super::*;

// Was ein Rechtsklick getroffen hat -- Vanillas BlockHitResult, auf die Felder reduziert,
// die ins Paket gehen (s. protocol::packets::use_item_on_payload).
#[derive(Clone, Copy)]
pub struct BlockRayHit {
    pub block: (i32, i32, i32),
    // Richtungs-Index wie im Vanilla-Enum: 0 unten, 1 oben, 2 Nord, 3 Sued, 4 West, 5 Ost.
    pub face: i32,
    // Trefferstelle RELATIV zur Blockecke (0..1 je Achse), so wie sie ins Paket geht.
    pub cursor: (f32, f32, f32),
    pub inside: bool,
}

// Strahl vom Auge in Blickrichtung gegen die Blockformen -- die Vorlage fuer den Rechtsklick
// (Vanillas World.raycast mit RaycastContext).
//
// TEILWEISE BEWUSSTE ABWEICHUNG: gemessen wird gegen world::outline_boxes -- Vanillas
// UMRISS-Form (ShapeType.OUTLINE), soweit wir sie fuehren. Bloecke, deren Kollisionsform
// kleiner ist als ihr Umriss (Seelensand, Honig), trifft der Strahl dadurch als vollen
// Wuerfel, genau wie in Vanilla. Was weiterhin abweicht, sind Bloecke ganz OHNE Kollision,
// aber mit Umriss (Blumen, hohes Gras): die zaehlen bei uns als begehbar und damit als
// umrisslos, wir schauen also durch sie hindurch auf den Boden dahinter, waehrend Vanilla sie
// selbst trifft. Beim Platzieren ist das Ergebnis meist identisch, weil solche Bloecke
// ersetzbar sind -- ausgeschlossen ist ein Unterschied aber nicht.
//
// Der Strahl misst gegen World::block_at, sieht die eigene Platzier-Vorhersage also MIT --
// wie Vanilla, das gegen seine ClientWorld samt Vorhersage misst. Damit laesst sich auf einen
// gerade gesetzten Block sofort weiterbauen, ohne auf die Bestaetigung zu warten.
//
// VORGESCHICHTE, damit niemand im Kreis laeuft: hier stand eine Zeitlang server_block_at, der
// Strahl ignorierte die eigene Vorhersage also bewusst. Anlass war eine Kette von GrimAC-Flags
// "AirLiquidPlace" (x0 bis x4 am Stueck) beim schnellen Bauen. Die Diagnose war unvollstaendig:
// im Grim-Jar (2.3.74) prueft AirLiquidPlace ausschliesslich, ob der Block, GEGEN
// den geklickt wurde, in GRIMS Welt (compensatedWorld) Luft oder Fluessigkeit ist -- und Grim
// fuehrt die Platzierungs-Vorhersage des Clients selbst mit (CheckManagerListener umschliesst
// jede Platzierung mit compensatedWorld.startPredicting()/stopPredicting(), BlockPlaceResult
// setzt den Block). Gegen einen frisch gesetzten eigenen Block zu bauen ist fuer Grim also
// voellig regulaer, solange die ERSTE Platzierung durchging.
//
// Geflaggt hat folglich nur der Fall, in dem die erste Platzierung ABGELEHNT wurde
// (geschuetztes Gebiet, Plugin, ein von Grim selbst gecanceltes Paket): dann steht bei uns ein
// Geisterblock, in Grims Welt aber Luft. Genau dieser Fall trifft einen Vanilla-Client
// identisch -- auch der baut gegen seinen eigenen, noch unbestaetigten Block. Dafuer das
// normale Bauen auszubremsen war der falsche Tausch (auf Geisterbloecken liess sich sonst nichts
// weiter platzieren).
//
// Die Vorhersage selbst bleibt bewusst konservativ (s. block_place::default_state_for_item:
// nur eindeutige Bloecke, nur auf ersetzbare Stellen, nie in den Spieler hinein) -- sie ist
// damit eine TEILMENGE dessen, was Vanilla vorhersagt, und kann keine Geisterbloecke erzeugen,
// die ein Vanilla-Client nicht auch haette.
pub fn raycast_block(eye: DVec3, dir: DVec3, reach: f64, world: &World, shapes: &world::BlockShapes) -> Option<BlockRayHit> {
    let to = eye + dir * reach;
    let region_min = eye.min(to);
    let region_max = eye.max(to);
    let (bx0, by0, bz0) = (region_min.x.floor() as i32, region_min.y.floor() as i32, region_min.z.floor() as i32);
    let (bx1, by1, bz1) = (region_max.x.floor() as i32, region_max.y.floor() as i32, region_max.z.floor() as i32);

    let ray = to - eye;
    let mut best: Option<(f64, BlockRayHit)> = None;

    for bx in bx0..=bx1 {
        for by in by0..=by1 {
            for bz in bz0..=bz1 {
                // block_at: inklusive eigener Platzier-Vorhersage, s. Kommentar ueber
                // raycast_block.
                let state = world.block_at(bx, by, bz);
                let (odx, odz) = world::block_name(state)
                    .map_or((0.0, 0.0), |name| world::outline_xz_offset(name, bx, bz));
                for cbox in world::outline_boxes(state, shapes) {
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
                    let Some((t, achse)) = slab_entry(eye, ray, bmin, bmax) else {
                        continue;
                    };
                    if best.as_ref().is_some_and(|(bt, _)| *bt <= t) {
                        continue;
                    }
                    let punkt = eye + ray * t.max(0.0);
                    // Die Seite, durch die der Strahl eintritt: laeuft er auf der Achse nach
                    // oben, trifft er die untere Seite -- und umgekehrt.
                    let face = match (achse, ray[achse] > 0.0) {
                        (0, true) => protocol::packets::FACE_WEST,
                        (0, false) => protocol::packets::FACE_EAST,
                        (1, true) => protocol::packets::FACE_DOWN,
                        (1, false) => protocol::packets::FACE_UP,
                        (_, true) => protocol::packets::FACE_NORTH,
                        (_, false) => protocol::packets::FACE_SOUTH,
                    };
                    best = Some((
                        t,
                        BlockRayHit {
                            block: (bx, by, bz),
                            face,
                            cursor: ((punkt.x - f64::from(bx)) as f32, (punkt.y - f64::from(by)) as f32, (punkt.z - f64::from(bz)) as f32),
                            inside: t < 0.0,
                        },
                    ));
                }
            }
        }
    }
    best.map(|(_, hit)| hit)
}

// Zuschauer-Flug: genau die eingestellte Geschwindigkeit in Richtung der gedrueckten Tasten,
// ab dem ersten Tick und ohne Nachlaufen. Waagerecht nach Blickrichtung gedreht wie Vanillas
// moveRelative, Springen hebt, Schleichen senkt; schraeg nicht schneller als geradeaus.
pub fn spectator_velocity(input: &Input) -> DVec3 {
    let (sin, cos) = f64::from(input.yaw_rad).sin_cos();
    let (vor, seit) = (f64::from(input.forward), f64::from(input.strafe));
    let hoch = f64::from(i32::from(input.jump)) - f64::from(i32::from(input.sneak));
    let v = DVec3::new(seit * cos - vor * sin, hoch, vor * cos + seit * sin);
    if v.length_squared() < 1.0e-9 {
        return DVec3::ZERO;
    }
    v.normalize() * input.spectator_speed
}

// Wie raycast_block, aber fuer weite Strecken (Zuschauer-Teleport, s. app::spectator): Zelle
// fuer Zelle entlang des Strahls statt aller Bloecke im ueberstrichenen Quader, und nur so weit,
// wie Spalten geladen sind. Steckt das Auge im Fels, zaehlt erst der Block hinter der ersten
// Luft -- sonst traefe man immer den Stein, in dem man steckt.
pub fn raycast_block_far(eye: DVec3, dir: DVec3, reach: f64, world: &World, shapes: &world::BlockShapes) -> Option<BlockRayHit> {
    let dir = dir.normalize_or_zero();
    if dir == DVec3::ZERO {
        return None;
    }
    let ray = dir * reach;
    let mut zelle = [eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32];
    let mut schritt = [0i32; 3];
    let mut naechste = [f64::INFINITY; 3];
    let mut abstand = [f64::INFINITY; 3];
    for a in 0..3 {
        if dir[a] > 0.0 {
            schritt[a] = 1;
            naechste[a] = (f64::from(zelle[a] + 1) - eye[a]) / dir[a];
            abstand[a] = 1.0 / dir[a];
        } else if dir[a] < 0.0 {
            schritt[a] = -1;
            naechste[a] = (f64::from(zelle[a]) - eye[a]) / dir[a];
            abstand[a] = -1.0 / dir[a];
        }
    }
    let unten = world.min_y();
    let lokal = eye - DVec3::new(f64::from(zelle[0]), f64::from(zelle[1]), f64::from(zelle[2]));
    let mut im_fels = world::outline_boxes(world.block_at(zelle[0], zelle[1], zelle[2]), shapes)
        .into_iter()
        .any(|b| (0..3).all(|a| f64::from(b.min[a]) <= lokal[a] && lokal[a] <= f64::from(b.max[a])));
    let mut t = 0.0;
    while t <= reach {
        let (bx, by, bz) = (zelle[0], zelle[1], zelle[2]);
        world.column(bx.div_euclid(16), bz.div_euclid(16))?;
        // Unter der Welt kommt nichts mehr; nach oben kann der Strahl wieder herunterkommen.
        if by < unten && dir.y <= 0.0 {
            return None;
        }
        let state = world.block_at(bx, by, bz);
        if state == world::STATE_AIR {
            im_fels = false;
        } else if !im_fels {
            let (odx, odz) = world::block_name(state).map_or((0.0, 0.0), |name| world::outline_xz_offset(name, bx, bz));
            let mut best: Option<(f64, usize)> = None;
            for cbox in world::outline_boxes(state, shapes) {
                let bmin = DVec3::new(f64::from(bx) + f64::from(odx) + f64::from(cbox.min[0]), f64::from(by) + f64::from(cbox.min[1]), f64::from(bz) + f64::from(odz) + f64::from(cbox.min[2]));
                let bmax = DVec3::new(f64::from(bx) + f64::from(odx) + f64::from(cbox.max[0]), f64::from(by) + f64::from(cbox.max[1]), f64::from(bz) + f64::from(odz) + f64::from(cbox.max[2]));
                if let Some((tb, achse)) = slab_entry(eye, ray, bmin, bmax) {
                    if tb >= 0.0 && best.is_none_or(|(bt, _)| tb < bt) {
                        best = Some((tb, achse));
                    }
                }
            }
            if let Some((tb, achse)) = best {
                let punkt = eye + ray * tb;
                let face = match (achse, ray[achse] > 0.0) {
                    (0, true) => protocol::packets::FACE_WEST,
                    (0, false) => protocol::packets::FACE_EAST,
                    (1, true) => protocol::packets::FACE_DOWN,
                    (1, false) => protocol::packets::FACE_UP,
                    (_, true) => protocol::packets::FACE_NORTH,
                    (_, false) => protocol::packets::FACE_SOUTH,
                };
                return Some(BlockRayHit {
                    block: (bx, by, bz),
                    face,
                    cursor: ((punkt.x - f64::from(bx)) as f32, (punkt.y - f64::from(by)) as f32, (punkt.z - f64::from(bz)) as f32),
                    inside: false,
                });
            }
        }
        // In die Nachbarzelle, deren Grenze der Strahl als naechste kreuzt.
        let a = if naechste[0] <= naechste[1] && naechste[0] <= naechste[2] {
            0
        } else if naechste[1] <= naechste[2] {
            1
        } else {
            2
        };
        t = naechste[a];
        naechste[a] += abstand[a];
        zelle[a] += schritt[a];
    }
    None
}

// Slab-Verfahren fuer EINE Box: liefert den Eintritts-Parameter auf der Strecke (kann
// negativ sein, wenn der Startpunkt schon drinsteckt) und die Achse, auf der der Strahl
// eintritt -- genau die bestimmt die getroffene Seite.
pub(crate) fn slab_entry(from: DVec3, ray: DVec3, bmin: DVec3, bmax: DVec3) -> Option<(f64, usize)> {
    let mut t_enter = f64::NEG_INFINITY;
    let mut t_exit = 1.0f64;
    let mut achse = 0usize;
    for a in 0..3 {
        let d = ray[a];
        if d.abs() < 1.0e-12 {
            // Parallel zur Achse: nur ein Treffer, wenn der Startpunkt schon im Schlitz liegt.
            if from[a] < bmin[a] || from[a] > bmax[a] {
                return None;
            }
            continue;
        }
        let (mut t0, mut t1) = ((bmin[a] - from[a]) / d, (bmax[a] - from[a]) / d);
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        if t0 > t_enter {
            t_enter = t0;
            achse = a;
        }
        t_exit = t_exit.min(t1);
        if t_enter > t_exit {
            return None;
        }
    }
    (t_enter.is_finite() && t_exit >= 0.0).then_some((t_enter, achse))
}

// Naechster Schnittpunkt der Strecke from->to mit einer Block-Kollisionsbox, oder None.
// Slab-Verfahren gegen die Boxen im ueberstrichenen Bereich; die Strecke ist hier nur
// wenige Bloecke lang (Kameradistanz), ein voller Voxel-Marsch waere Aufwand ohne Gewinn.
pub(crate) fn raycast_blocks(from: DVec3, to: DVec3, world: &World, shapes: &world::BlockShapes) -> Option<DVec3> {
    let dir = to - from;
    let len = dir.length();
    if len < 1.0e-9 {
        return None;
    }
    let region_min = from.min(to);
    let region_max = from.max(to);
    let boxes = collect_camera_boxes(region_min, region_max, world, shapes);

    let mut nearest: Option<f64> = None;
    for b in &boxes {
        let mut t_enter = 0.0f64;
        let mut t_exit = 1.0f64;
        let mut hit = true;
        for axis in 0..3 {
            let d = dir[axis];
            let (lo, hi) = (b.min[axis], b.max[axis]);
            if d.abs() < 1.0e-12 {
                // Parallel zur Achse: nur ein Treffer, wenn der Startpunkt schon im Schlitz liegt.
                if from[axis] < lo || from[axis] > hi {
                    hit = false;
                    break;
                }
                continue;
            }
            let mut t0 = (lo - from[axis]) / d;
            let mut t1 = (hi - from[axis]) / d;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
            }
            t_enter = t_enter.max(t0);
            t_exit = t_exit.min(t1);
            if t_enter > t_exit {
                hit = false;
                break;
            }
        }
        if hit && t_enter >= 0.0 {
            nearest = Some(nearest.map_or(t_enter, |n: f64| n.min(t_enter)));
        }
    }
    nearest.map(|t| from + dir * t)
}

pub(crate) fn move_and_collide(
    pos: DVec3,
    vel: DVec3,
    height: f64,
    on_ground: bool,
    world: &World,
    shapes: &world::BlockShapes,
    ctx: world::ShapeContext,
    body: BodyAttributes,
) -> (DVec3, bool, bool, bool, bool) {
    let half = body.width() / 2.0;
    let box_min = DVec3::new(pos.x - half, pos.y, pos.z - half);
    let box_max = DVec3::new(pos.x + half, pos.y + height, pos.z + half);

    let moved = adjust_movement_with_step(box_min, box_max, vel, on_ground, world, shapes, ctx, body.step_height);

    let hit_x = (moved.x - vel.x).abs() > 1.0e-6;
    let hit_z = (moved.z - vel.z).abs() > 1.0e-6;
    // Ein geglueckter Schritt zaehlt bewusst als Vertikalkollision nach unten: die Bewegung
    // ging statt nach unten nach OBEN, was Vanilla ebenso als "movement.y != vec3d.y" sieht
    // und ueber Block.onEntityLand die Y-Geschwindigkeit nullt. Ohne das behielte man den
    // Fallimpuls und saenke im naechsten Tick wieder von der Stufe.
    let hit_y = (moved.y - vel.y).abs() > 1.0e-6;
    // Vanillas groundCollision: `verticalCollision && movement.y < 0.0` (Entity.move,
    // Zeile 764) -- STRIKT kleiner null. Bei movement.y == 0 (z.B. der erste Tick nach einem
    // Teleport, dessen Geschwindigkeit genullt wurde) zaehlt ein Treffer NICHT als
    // Bodenkontakt, auch wenn die Stufenlogik dabei ein Stueck nach oben geschoben hat.
    let hit_y_down = hit_y && vel.y < 0.0;
    let hit_y_up = hit_y && vel.y > 0.0;

    (pos + moved, hit_x, hit_y_down, hit_y_up, hit_z)
}
