// Kollisionsboxen und Bewegung mit Kollision und Stufen.

use super::*;

// Bewegt die Spieler-Boundingbox achsenweise (Y, dann X, dann Z) gegen die echten
// Kollisionsboxen der ueberlappenden Bloecke (s. world::collision_boxes -- ein Vollwuerfel
// fuer die meisten Terrain-Bloecke, aber z.B. nur die halbe Hoehe fuer eine Stufe), klemmt
// jede Achse einzeln an der ersten Kollision. Gibt die neue Fuessposition und je Achse
// zurueck, ob dort eine Kollision die Bewegung verkuerzt hat (down/up getrennt fuer Y, weil
// nur "nach unten blockiert" == Bodenkontakt bedeutet).
// Eine achsenparallele Kollisionsbox in WELTkoordinaten (Blockposition + xz-Offset schon
// eingerechnet, s. collect_collision_boxes) -- Vanillas VoxelShape-Liste entspricht hier
// einer flachen Liste solcher Boxen.
#[derive(Clone, Copy)]
pub(crate) struct Aabb {
    pub(crate) min: DVec3,
    pub(crate) max: DVec3,
}

// Box.stretch(dx,dy,dz): die Box in Bewegungsrichtung aufziehen, ohne die Gegenseite zu
// verschieben -- der Bereich, den die Box auf ihrem Weg ueberstreicht.
pub(crate) fn stretch(min: DVec3, max: DVec3, delta: DVec3) -> (DVec3, DVec3) {
    let (mut lo, mut hi) = (min, max);
    for axis in 0..3 {
        if delta[axis] < 0.0 {
            lo[axis] += delta[axis];
        } else {
            hi[axis] += delta[axis];
        }
    }
    (lo, hi)
}

// Alle Block-Kollisionsboxen, die den Bereich [min,max] tatsaechlich schneiden -- Vanillas
// World.getBlockCollisions(entity, box). EINMAL eingesammelt statt pro Achse neu gescannt:
// Vanillas Step-Up befragt denselben Satz Boxen mehrfach (adjustMovementForCollisions laeuft
// je Kandidatenhoehe erneut ueber dieselbe Liste `list2`), und die Kandidatenhoehen selbst
// werden ebenfalls aus genau dieser Liste gezogen.
// Entity.adjustMovementForCollisions(null, movement, box, world, List.of()) -- die Fassung
// OHNE Entity und ohne Stufensteigen, mit der Partikel sich bewegen (s. particles.rs).
pub fn collide_box(min: DVec3, max: DVec3, movement: DVec3, world: &World, shapes: &world::BlockShapes) -> DVec3 {
    let (lo, hi) = stretch(min, max, movement);
    let boxes = collect_collision_boxes(lo, hi, world, shapes, world::ShapeContext::ABSENT);
    adjust_movement(min, max, movement, &boxes)
}

// --- Fuer andere Koerper als den Spieler (s. vehicle.rs) ------------------------------------

// Steht IRGENDEIN Block mit seiner Kollisionsform in der Box? World.isSpaceEmpty ohne Entities.
pub(crate) fn space_empty(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) -> bool {
    collect_collision_boxes(min, max, world, shapes, world::ShapeContext::ABSENT).is_empty()
}

// Schneidet die Kollisionsform des Blocks (bx, by, bz) die Box mit echtem Volumen?
// VoxelShapes.matchesAnywhere(Form, Box, AND).
pub(crate) fn block_shape_overlaps(bx: i32, by: i32, bz: i32, min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) -> bool {
    let state = world.block_at(bx, by, bz);
    let (odx, odz) = world::block_name(state).map_or((0.0, 0.0), |name| world::block_xz_offset(name, bx, bz));
    world::collision_boxes_at(state, shapes, by, world::ShapeContext::ABSENT).iter().any(|b| {
        let lo = DVec3::new(f64::from(bx) + f64::from(odx) + f64::from(b.min[0]), f64::from(by) + f64::from(b.min[1]), f64::from(bz) + f64::from(odz) + f64::from(b.min[2]));
        let hi = DVec3::new(f64::from(bx) + f64::from(odx) + f64::from(b.max[0]), f64::from(by) + f64::from(b.max[1]), f64::from(bz) + f64::from(odz) + f64::from(b.max[2]));
        lo.x < max.x && hi.x > min.x && lo.y < max.y && hi.y > min.y && lo.z < max.z && hi.z > min.z
    })
}

// Block.getSlipperiness je Blockname (s. block_friction).
pub(crate) fn friction_of(name: &str) -> f64 {
    block_friction(name)
}

pub(crate) fn velocity_multiplier_of(name: &str) -> f64 {
    block_velocity_multiplier(name)
}

pub(crate) fn bounce_restitution_of(name: &str) -> f64 {
    block_bounce_restitution(name)
}

// Entity.updateMovementInFluid(WATER, 0.014) fuer alles, was kein Spieler ist: wie
// apply_fluid_push, nur wird der gemittelte Stroemungsvektor vorher auf Laenge 1 gebracht.
// Liefert, ob die Box Wasser beruehrt.
pub(crate) fn push_by_water(min: DVec3, max: DVec3, vel: &mut DVec3, world: &World, shapes: &world::BlockShapes) -> bool {
    let Some(scan) = fluid_height_scan(min, max, world, shapes, world::FluidKind::Water) else { return false };
    if scan.push_cells > 0 && scan.push.length() > 0.0 {
        let gemittelt = (scan.push / f64::from(scan.push_cells)).normalize_or_zero();
        let normiert = FluidScan { height: scan.height, push: gemittelt, push_cells: 1 };
        apply_fluid_push(vel, &normiert, WATER_PUSH_SPEED);
    }
    true
}

thread_local! {
    // PistonMovingBlockEntity.NOCLIP: die Richtung, in die ein Kolben gerade schiebt, solange
    // er schiebt (s. piston_move). Bloecke, die in genau diese Richtung fahren, halten den
    // Geschobenen dabei nicht auf -- sonst stiesse er sich an dem Block, der ihn schiebt.
    static PISTON_NOCLIP: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

// Fuer die Dauer von `f` gilt `richtung` als NOCLIP-Richtung (s. PISTON_NOCLIP).
pub fn with_piston_noclip<R>(richtung: u8, f: impl FnOnce() -> R) -> R {
    PISTON_NOCLIP.with(|c| c.set(Some(richtung)));
    let r = f();
    PISTON_NOCLIP.with(|c| c.set(None));
    r
}

pub(crate) fn collect_collision_boxes(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes, ctx: world::ShapeContext) -> Vec<Aabb> {
    let mut out = collect_boxes(min, max, world, &|state, by| world::collision_boxes_at(state, shapes, by, ctx));
    // Vom Kolben bewegte Bloecke: ihre Zelle (moving_piston) hat keine Form, die Boxen haengen
    // an der Bewegung (s. World::moving_boxes).
    let noclip = PISTON_NOCLIP.with(std::cell::Cell::get);
    for b in &world.moving_boxes {
        if b.noclip.is_some() && b.noclip == noclip {
            continue;
        }
        let (bmin, bmax) = (DVec3::from_array(b.min), DVec3::from_array(b.max));
        if bmin.x < max.x && bmax.x > min.x && bmin.y < max.y && bmax.y > min.y && bmin.z < max.z && bmax.z > min.z {
            out.push(Aabb { min: bmin, max: bmax });
        }
    }
    out
}

// Dasselbe fuer die Verfolgerkamera: Vanilla misst sie gegen getCameraCollisionShape, die sich
// nur bei Seelensand von der Kollisionsform unterscheidet (dort Vollwuerfel, s.
// world::camera_collision_boxes) -- ohne das saenke die Kamera in den Sand ein, in dem der
// Spieler steht.
pub(crate) fn collect_camera_boxes(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) -> Vec<Aabb> {
    collect_boxes(min, max, world, &|state, _| world::camera_collision_boxes(state, shapes))
}

pub(crate) fn collect_boxes(min: DVec3, max: DVec3, world: &World, boxes_of: &dyn Fn(u32, i32) -> Vec<world::ShapeBox>) -> Vec<Aabb> {
    let bx0 = min.x.floor() as i32;
    let bx1 = (max.x - EPS).floor() as i32;
    // Y nach unten um MAX_COLLISION_OVERFLOW_BLOCKS erweitert: ein Zaun/eine Mauer/ein Tor
    // steht in Zelle Y, seine Kollisionsbox reicht aber bis Y+1.5 (s. vanilla_shapes.rs) --
    // ragt also in die Zelle DARUEBER hinein. Ohne die Erweiterung faellt genau diese
    // Zelle (Y) aus dem Scan, sobald der Spieler hoch genug ist (z.B. mitten im Sprung),
    // und man laeuft/springt mitten durch den eigentlich noch blockierenden oberen Teil --
    // beobachtet als "kann ueber den Zaun laufen, wird vom Server dafuer zurueckgesetzt".
    let by0 = (min.y - MAX_COLLISION_OVERFLOW_BLOCKS).floor() as i32;
    let by1 = (max.y - EPS).floor() as i32;
    let bz0 = min.z.floor() as i32;
    let bz1 = (max.z - EPS).floor() as i32;

    let mut out = Vec::new();
    for bx in bx0..=bx1 {
        for by in by0..=by1 {
            for bz in bz0..=bz1 {
                let state = world.block_at(bx, by, bz);
                // Gegen minecraft.wiki/w/Block_offset verifiziert (s. world::block_offset-
                // Kommentar): getOffset() verschiebt in Vanilla NICHT nur die Renderposition,
                // sondern auch die Kollisionsbox -- ohne das kollidiert z.B. Bambus immer an
                // der Zellenmitte statt an seiner tatsaechlichen (leicht schiefen) Position,
                // waehrend der echte Server dort schon verschoben kollidiert (Server-Flag
                // beim Durchlaufen von Bambus).
                let (odx, odz) = world::block_name(state)
                    .map_or((0.0, 0.0), |name| world::block_xz_offset(name, bx, bz));
                for cbox in boxes_of(state, by) {
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
                    // Nur Boxen, die den gesuchten Bereich wirklich schneiden. Ohne diesen
                    // Filter kaemen aus einer nur teilweise erfassten Zelle Boxen mit, die
                    // gar nicht im Weg liegen -- fuer die Achsen-Klemmung harmlos, fuer die
                    // Stufen-Kandidaten aber nicht: die zieht ihre Hoehen aus genau dieser
                    // Liste und wuerde sonst auf Kanten steigen, die nirgends beruehrt werden.
                    if bmin.x < max.x && bmax.x > min.x && bmin.y < max.y && bmax.y > min.y && bmin.z < max.z && bmax.z > min.z {
                        out.push(Aabb { min: bmin, max: bmax });
                    }
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}

// Direction.getCollisionOrder(): Y zuerst, danach die betragsmaessig KLEINERE Horizontale,
// zuletzt die groessere (`|x| < |z| ? YZX : YXZ`). Das ist keine Kosmetik -- in einer
// Innenecke gibt erst die eine Achse die andere frei, das Ergebnis haengt also an der
// Reihenfolge. Vorher stand hier fest Y,X,Z.
pub(crate) fn collision_order(movement: DVec3) -> [Axis; 3] {
    if movement.x.abs() < movement.z.abs() {
        [Axis::Y, Axis::Z, Axis::X]
    } else {
        [Axis::Y, Axis::X, Axis::Z]
    }
}

// VoxelShapes.calculateMaxOffset fuer EINE Achse: wie weit sich [min,max] entlang `axis` um
// `delta` bewegen laesst, bevor sie in eine der Boxen liefe. Nur Boxen, die auf den beiden
// ANDEREN Achsen mit der Spieler-Box ueberlappen, koennen ueberhaupt blockieren.
pub(crate) fn clamp_axis(min: DVec3, max: DVec3, delta: f64, axis: Axis, boxes: &[Aabb]) -> f64 {
    if delta.abs() < 1.0e-9 {
        return 0.0;
    }
    let mut allowed = delta;
    for b in boxes {
        let (bmin, bmax) = (b.min, b.max);
        let overlaps_other_axes = match axis {
            Axis::X => min.y < bmax.y && max.y > bmin.y && min.z < bmax.z && max.z > bmin.z,
            Axis::Y => min.x < bmax.x && max.x > bmin.x && min.z < bmax.z && max.z > bmin.z,
            Axis::Z => min.x < bmax.x && max.x > bmin.x && min.y < bmax.y && max.y > bmin.y,
        };
        if !overlaps_other_axes {
            continue;
        }

        let (cur_min, cur_max, box_min, box_max) = match axis {
            Axis::X => (min.x, max.x, bmin.x, bmax.x),
            Axis::Y => (min.y, max.y, bmin.y, bmax.y),
            Axis::Z => (min.z, max.z, bmin.z, bmax.z),
        };

        if delta > 0.0 && box_min >= cur_max - EPS {
            allowed = allowed.min((box_min - cur_max).max(0.0));
        } else if delta < 0.0 && box_max <= cur_min + EPS {
            allowed = allowed.max(-((cur_min - box_max).max(0.0)));
        }
    }
    allowed
}

// Entity.adjustMovementForCollisions(Vec3d, Box, List): die drei Achsen NACHEINANDER klemmen,
// jede gegen die um das bisher Erreichte verschobene Box.
//
// Die Reihenfolge kommt aus collision_order() -- s. dortigen Kommentar.
pub(crate) fn adjust_movement(min: DVec3, max: DVec3, movement: DVec3, boxes: &[Aabb]) -> DVec3 {
    if boxes.is_empty() {
        return movement;
    }
    let mut result = DVec3::ZERO;
    for axis in collision_order(movement) {
        let d = match axis {
            Axis::X => movement.x,
            Axis::Y => movement.y,
            Axis::Z => movement.z,
        };
        if d == 0.0 {
            continue;
        }
        let e = clamp_axis(min + result, max + result, d, axis, boxes);
        match axis {
            Axis::X => result.x = e,
            Axis::Y => result.y = e,
            Axis::Z => result.z = e,
        }
    }
    result
}

// Entity.collectStepHeights: die in Frage kommenden Stufenhoehen, aufsteigend sortiert und
// ohne Dubletten. Kandidat ist jede Y-Kante einer Kollisionsbox, relativ zur Unterkante der
// Spielerbox -- also genau "wie hoch muesste ich steigen, damit meine Fuesse auf dieser
// Kante stehen".
//
// Zwei Vanilla-Details, die leicht untergehen:
//  * Die Hoehen werden in FLOAT gerechnet und verglichen (Vanilla rechnet den Abstand
//    zur Box-Unterkante als float), nicht in double. Fuer Grim muss die resultierende Bewegung
//    bit-genau passen, deshalb hier ebenso f32.
//  * Die Hoehe, die die Basisbewegung ohnehin schon erreicht (base_dy), faellt raus -- sie
//    wuerde dieselbe Bewegung nur nochmal liefern.
// Vanilla bricht die Schleife bei der ersten Hoehe ueber der Stufenhoehe ab (seine
// Punktliste ist aufsteigend); bei unserer unsortierten Boxliste ist das gleichbedeutend
// mit "solche Hoehen weglassen".
pub(crate) fn collect_step_heights(box_min_y: f64, boxes: &[Aabb], step_height: f64, base_dy: f64) -> Vec<f32> {
    let exclude = base_dy as f32;
    let limit = step_height as f32;
    let mut out: Vec<f32> = Vec::new();
    for b in boxes {
        for edge in [b.min.y, b.max.y] {
            let g = (edge - box_min_y) as f32;
            if g < 0.0 || g == exclude || g > limit {
                continue;
            }
            if !out.contains(&g) {
                out.push(g);
            }
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).expect("Stufenhöhen sind nie NaN"));
    out
}

// Die Weltgrenze als Wand dazu, sobald der Koerper (Mitte aus box_min/box_max) nahe genug
// davor steht -- gemessen an der ueberstrichenen Box swept_min/swept_max (s.
// world::world_border::WorldBorder::collision_boxes). Ihre Kanten liegen weit ober- und
// unterhalb jeder Stufenhoehe und fallen bei den Stufen-Kandidaten deshalb heraus.
pub(crate) fn add_border_boxes(boxes: &mut Vec<Aabb>, box_min: DVec3, box_max: DVec3, swept_min: DVec3, swept_max: DVec3, world: &World) {
    let mitte = (box_min + box_max) * 0.5;
    let ausdehnung = (swept_max.x - swept_min.x, swept_max.z - swept_min.z);
    for (min, max) in world.border.collision_boxes(mitte.x, mitte.z, ausdehnung) {
        boxes.push(Aabb { min: DVec3::from_array(min), max: DVec3::from_array(max) });
    }
}

pub(crate) fn horizontal_length_squared(v: DVec3) -> f64 {
    v.x * v.x + v.z * v.z
}

// Entity.adjustMovementForCollisions(Vec3d): Basisbewegung klemmen und, falls sie horizontal
// blockiert wurde, versuchen, das Hindernis zu ERSTEIGEN statt daran haengenzubleiben.
//
// Genau das ist Minecrafts "Stepping": Stufen, Slabs, Teppiche und jede andere Kante bis
// STEP_HEIGHT ueberwindet man im Laufen, ohne zu springen. Die Bedingung dafuer ist eng --
// nur wenn man am Boden ist (oder in genau diesem Tick gelandet ist) UND horizontal
// angestossen ist. Sonst koennte man an einer Wand hochlaufen.
pub(crate) fn adjust_movement_with_step(
    box_min: DVec3,
    box_max: DVec3,
    movement: DVec3,
    on_ground: bool,
    world: &World,
    shapes: &world::BlockShapes,
    ctx: world::ShapeContext,
    step_height: f64,
) -> DVec3 {
    let (r_min, r_max) = stretch(box_min, box_max, movement);
    let mut boxes = collect_collision_boxes(r_min, r_max, world, shapes, ctx);
    add_border_boxes(&mut boxes, box_min, box_max, r_min, r_max, world);
    let base = if movement.length_squared() == 0.0 { movement } else { adjust_movement(box_min, box_max, movement, &boxes) };

    let clipped_x = movement.x != base.x;
    let clipped_y = movement.y != base.y;
    let clipped_z = movement.z != base.z;
    // "In diesem Tick auf etwas gelandet" -- dann wird von der GELANDETEN Hoehe aus
    // gestiegen, nicht von der Ausgangshoehe.
    let landed = clipped_y && movement.y < 0.0;

    if !((landed || on_ground) && (clipped_x || clipped_z)) {
        return base;
    }

    let lift = if landed { DVec3::new(0.0, base.y, 0.0) } else { DVec3::ZERO };
    let (b2_min, b2_max) = (box_min + lift, box_max + lift);

    let (mut s_min, s_max) = stretch(b2_min, b2_max, DVec3::new(movement.x, step_height, movement.z));
    // Ohne Landung in diesem Tick zusaetzlich ein Hauch nach unten (Vanilla: stretch(0,
    // -1.0E-5, 0)) -- sonst faende der Scan die Box, auf der man GERADE STEHT, nicht mehr,
    // und ihre Oberkante fiele als Kandidat weg.
    if !landed {
        s_min.y -= 1.0e-5;
    }
    let mut step_boxes = collect_collision_boxes(s_min, s_max, world, shapes, ctx);
    add_border_boxes(&mut step_boxes, box_min, box_max, s_min, s_max, world);

    for g in collect_step_heights(b2_min.y, &step_boxes, step_height, base.y) {
        let attempt = DVec3::new(movement.x, f64::from(g), movement.z);
        let stepped = adjust_movement(b2_min, b2_max, attempt, &step_boxes);
        // Nur uebernehmen, wenn man dadurch horizontal WEITER kommt -- eine Stufe, die einen
        // nur anhebt, ohne das Hindernis zu loesen, waere ein Aufzug an der Wand.
        if horizontal_length_squared(stepped) > horizontal_length_squared(base) {
            // Ergebnis ist relativ zu box2; bei einer Landung lag die schon um base.y
            // tiefer, das muss wieder drauf.
            return stepped - DVec3::new(0.0, box_min.y - b2_min.y, 0.0);
        }
    }
    base
}

// Vanillas Camera.clipToSpace(): wie weit die Kamera in der 3.-Person-Ansicht wirklich nach
// hinten darf, ohne in eine Wand zu rutschen. Acht Strahlen von den Ecken eines kleinen
// Wuerfels um das Auge (+-0.1 je Achse) entlang `back_dir`; der naechste Treffer verkuerzt
// die Distanz. Die acht Ecken statt eines einzelnen Strahls sind der Grund, warum Vanillas
// Kamera an Ecken nicht durch die Wand blitzt.
//
// Vanilla misst gegen ShapeType.VISUAL (die Umriss-Form); hier sind es die Kollisionsboxen --
// fuer Vollbloecke identisch, bei Sonderformen (Zaun, Treppe) einen Hauch anders. Der
// Unterschied ist rein optisch: die Kamera ist Client-Sache, kein Server sieht sie.
pub fn clip_camera_distance(eye: DVec3, back_dir: DVec3, distance: f64, world: &World, shapes: &world::BlockShapes) -> f64 {
    let mut distance = distance;
    for i in 0..8 {
        let g = f64::from((i & 1) * 2 - 1);
        let h = f64::from((i >> 1 & 1) * 2 - 1);
        let j = f64::from((i >> 2 & 1) * 2 - 1);
        let from = eye + DVec3::new(g * 0.1, h * 0.1, j * 0.1);
        let to = from + back_dir * distance;
        let Some(hit) = raycast_blocks(from, to, world, shapes) else {
            continue;
        };
        // Gemessen wird ab dem AUGE, nicht ab der (leicht versetzten) Strahl-Ecke -- wie
        // Vanilla (Abstandsquadrat vom Trefferpunkt zur eigenen Position).
        let k = (hit - eye).length_squared();
        if k < distance * distance {
            distance = k.sqrt();
        }
    }
    distance
}
