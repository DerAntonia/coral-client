// Kolben auf dem Client.
//
// Der Server schickt fuer jeden Kolbenhub nur ein Block-Event (Typ 0 = ausfahren, 1/2 =
// einfahren, Daten = Blickrichtung). Den Hub selbst rechnet der Vanilla-Client nach: er sucht
// dieselben Bloecke zusammen wie der Server, setzt an ihre Zielzellen moving_piston und laesst
// die Bloecke in zwei Ticks dorthin fahren. Dabei schiebt er Entities -- den EIGENEN Spieler
// eingeschlossen, dessen Position der Server ja vom Client uebernimmt. Ein Schleimblock setzt
// ausserdem die Geschwindigkeit auf der Bewegungsachse auf 1: das Schleudern. Ohne diese
// Nachrechnung blieb man vor jedem Kolben einfach stehen.
//
// Nachgebaut sind PistonBaseBlock.triggerEvent/moveBlocks, PistonStructureResolver und
// PistonMovingBlockEntity (Tick, Kollisionsform, Schub). Welche Bloecke sich schieben lassen,
// steht in piston_data.txt (aus dem Jar gedumpt). Andere Entities schiebt nur der Server --
// ihre Positionen kommen ohnehin von dort.

use std::collections::HashMap;
use std::sync::OnceLock;

use glam::DVec3;
use world::World;

use crate::block_place::state_with;
use crate::physics::PlayerPhysics;

type Pos = (i32, i32, i32);

// Richtungen in Vanillas 3D-Datenwert-Reihenfolge (das Block-Event traegt genau diesen Wert).
const STEP: [Pos; 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];
const NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const DOWN: u8 = 0;
const UP: u8 = 1;

// Hoechstens so viele Bloecke schiebt ein Kolben.
const MAX_PUSH: usize = 12;

fn opposite(d: u8) -> u8 {
    d ^ 1
}

fn axis(d: u8) -> usize {
    match d {
        0 | 1 => 1,
        2 | 3 => 2,
        _ => 0,
    }
}

fn rel(p: Pos, d: u8, n: i32) -> Pos {
    let s = STEP[d as usize];
    (p.0 + s.0 * n, p.1 + s.1 * n, p.2 + s.2 * n)
}

fn step_vec(d: u8) -> DVec3 {
    let s = STEP[d as usize];
    DVec3::new(s.0 as f64, s.1 as f64, s.2 as f64)
}

fn direction_of(state: u32) -> Option<u8> {
    let f = world::state_props::property(state, "facing")?;
    NAMES.iter().position(|n| *n == f).map(|i| i as u8)
}

fn name(state: u32) -> &'static str {
    world::block_name(state).unwrap_or("")
}

fn is_air(state: u32) -> bool {
    matches!(name(state), "air" | "cave_air" | "void_air")
}

fn is_piston_base(state: u32) -> bool {
    matches!(name(state), "piston" | "sticky_piston")
}

fn is_sticky(state: u32) -> bool {
    matches!(name(state), "slime_block" | "honey_block")
}

// Schleim und Honig kleben nicht aneinander, alles andere klebt, sobald einer von beiden klebt.
fn can_stick(a: u32, b: u32) -> bool {
    match (name(a), name(b)) {
        ("honey_block", "slime_block") | ("slime_block", "honey_block") => false,
        _ => is_sticky(a) || is_sticky(b),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reaction {
    Normal,
    Destroy,
    Block,
    PushOnly,
}

// Blockname -> (Reaktion, hat Block-Entity), s. piston_data.txt.
fn rules() -> &'static HashMap<&'static str, (Reaction, bool)> {
    static RULES: OnceLock<HashMap<&'static str, (Reaction, bool)>> = OnceLock::new();
    RULES.get_or_init(|| {
        include_str!("piston_data.txt")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let mut teile = l.split_whitespace();
                let name = teile.next()?;
                let reaktion = match teile.next()? {
                    "D" => Reaction::Destroy,
                    "B" => Reaction::Block,
                    "P" => Reaction::PushOnly,
                    _ => Reaction::Normal,
                };
                Some((name, (reaktion, teile.next()? == "1")))
            })
            .collect()
    })
}

fn reaction(state: u32) -> Reaction {
    rules().get(name(state)).map_or(Reaction::Normal, |r| r.0)
}

fn has_block_entity(state: u32) -> bool {
    rules().get(name(state)).is_some_and(|r| r.1)
}

// Der erste State eines Blocks mit den genannten Properties (die uebrigen wie im ersten State).
fn state_of(block: &str, props: &[(&str, &str)]) -> Option<u32> {
    static FIRST: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    let first = FIRST.get_or_init(|| {
        let mut m = HashMap::new();
        for s in 0..world::state_count() as u32 {
            if let Some(n) = world::block_name(s) {
                m.entry(n).or_insert(s);
            }
        }
        m
    });
    let mut s = *first.get(block)?;
    for (k, v) in props {
        s = state_with(s, k, v)?;
    }
    Some(s)
}

fn air() -> u32 {
    world::STATE_AIR
}

fn moving_piston(facing: u8, sticky: bool) -> Option<u32> {
    state_of("moving_piston", &[("facing", NAMES[facing as usize]), ("type", if sticky { "sticky" } else { "normal" })])
}

fn piston_head(facing: u8, sticky: bool, short: bool) -> Option<u32> {
    state_of(
        "piston_head",
        &[("facing", NAMES[facing as usize]), ("type", if sticky { "sticky" } else { "normal" }), ("short", if short { "true" } else { "false" })],
    )
}

// Oberste Blockreihe der Dimension an dieser Stelle (Level.getMaxY).
fn max_y(world: &World, x: i32, z: i32) -> i32 {
    let sections = world.column(x.div_euclid(16), z.div_euclid(16)).map_or(24, |c| c.sections.len() as i32);
    world.min_y() + sections * 16 - 1
}

// PistonBaseBlock.isPushable.
fn is_pushable(world: &World, state: u32, pos: Pos, move_dir: u8, allow_destroy: bool, piston_facing: u8) -> bool {
    let top = max_y(world, pos.0, pos.2);
    if pos.1 < world.min_y() || pos.1 > top || !world.border.is_within_bounds(pos.0 as f64, pos.2 as f64, 0.0) {
        return false;
    }
    if is_air(state) {
        return true;
    }
    if matches!(name(state), "obsidian" | "crying_obsidian" | "respawn_anchor" | "reinforced_deepslate") {
        return false;
    }
    if (move_dir == DOWN && pos.1 == world.min_y()) || (move_dir == UP && pos.1 == top) {
        return false;
    }
    if is_piston_base(state) {
        if world::state_props::property(state, "extended") == Some("true") {
            return false;
        }
    } else {
        if world::mining::hardness(state) == -1.0 {
            return false;
        }
        match reaction(state) {
            Reaction::Block => return false,
            Reaction::Destroy => return allow_destroy,
            Reaction::PushOnly => return move_dir == piston_facing,
            Reaction::Normal => {}
        }
    }
    !has_block_entity(state)
}

// PistonStructureResolver: welche Bloecke ein Hub bewegt und welche er zerbricht.
struct Resolver<'a> {
    world: &'a World,
    piston: Pos,
    piston_facing: u8,
    push: u8,
    start: Pos,
    to_push: Vec<Pos>,
    to_destroy: Vec<Pos>,
    extending: bool,
}

impl<'a> Resolver<'a> {
    fn new(world: &'a World, piston: Pos, facing: u8, extending: bool) -> Self {
        let (push, start) = if extending { (facing, rel(piston, facing, 1)) } else { (opposite(facing), rel(piston, facing, 2)) };
        Self { world, piston, piston_facing: facing, push, start, to_push: Vec::new(), to_destroy: Vec::new(), extending }
    }

    fn at(&self, p: Pos) -> u32 {
        self.world.block_at(p.0, p.1, p.2)
    }

    fn resolve(&mut self) -> bool {
        self.to_push.clear();
        self.to_destroy.clear();
        let s = self.at(self.start);
        if !is_pushable(self.world, s, self.start, self.push, false, self.piston_facing) {
            if self.extending && reaction(s) == Reaction::Destroy {
                self.to_destroy.push(self.start);
                return true;
            }
            return false;
        }
        if !self.add_block_line(self.start, self.push) {
            return false;
        }
        let mut i = 0;
        while i < self.to_push.len() {
            let p = self.to_push[i];
            if is_sticky(self.at(p)) && !self.add_branching_blocks(p) {
                return false;
            }
            i += 1;
        }
        true
    }

    // Eine Linie ab `pos`: erst rueckwaerts alles, was an einem klebenden Block haengt, dann
    // vorwaerts alles, was davor im Weg steht.
    fn add_block_line(&mut self, pos: Pos, dir: u8) -> bool {
        let mut s = self.at(pos);
        if is_air(s) || !is_pushable(self.world, s, pos, self.push, false, dir) || pos == self.piston || self.to_push.contains(&pos) {
            return true;
        }
        let back = opposite(self.push);
        let mut i = 1;
        if i + self.to_push.len() > MAX_PUSH {
            return false;
        }
        while is_sticky(s) {
            let p = rel(pos, back, i as i32);
            let vorher = s;
            s = self.at(p);
            if is_air(s) || !can_stick(vorher, s) || !is_pushable(self.world, s, p, self.push, false, back) || p == self.piston {
                break;
            }
            i += 1;
            if i + self.to_push.len() > MAX_PUSH {
                return false;
            }
        }
        let mut j = 0;
        for k in (0..i).rev() {
            self.to_push.push(rel(pos, back, k as i32));
            j += 1;
        }
        let mut k = 1;
        loop {
            let p = rel(pos, self.push, k);
            if let Some(l) = self.to_push.iter().position(|q| *q == p) {
                self.reorder_at_collision(j, l);
                for m in 0..=l + j {
                    let q = self.to_push[m];
                    if is_sticky(self.at(q)) && !self.add_branching_blocks(q) {
                        return false;
                    }
                }
                return true;
            }
            let s = self.at(p);
            if is_air(s) {
                return true;
            }
            if !is_pushable(self.world, s, p, self.push, true, self.push) || p == self.piston {
                return false;
            }
            if reaction(s) == Reaction::Destroy {
                self.to_destroy.push(p);
                return true;
            }
            if self.to_push.len() >= MAX_PUSH {
                return false;
            }
            self.to_push.push(p);
            j += 1;
            k += 1;
        }
    }

    // Laeuft eine neue Linie in eine schon erfasste hinein, kommen ihre Bloecke VOR deren Rest.
    fn reorder_at_collision(&mut self, j: usize, l: usize) {
        let n = self.to_push.len();
        let mut neu = Vec::with_capacity(n);
        neu.extend_from_slice(&self.to_push[..l]);
        neu.extend_from_slice(&self.to_push[n - j..]);
        neu.extend_from_slice(&self.to_push[l..n - j]);
        self.to_push = neu;
    }

    // Ein klebender Block nimmt seine Nachbarn quer zur Schubrichtung mit.
    fn add_branching_blocks(&mut self, pos: Pos) -> bool {
        let s = self.at(pos);
        for d in 0..6u8 {
            if axis(d) == axis(self.push) {
                continue;
            }
            let p = rel(pos, d, 1);
            if can_stick(self.at(p), s) && !self.add_block_line(p, d) {
                return false;
            }
        }
        true
    }
}

// Ein Block auf der Fahrt (PistonMovingBlockEntity). `pos` ist die ZIELzelle, in der
// moving_piston steht; `facing` die Blickrichtung des Kolbens (nicht die Fahrtrichtung, die
// beim Einfahren entgegengesetzt ist).
#[derive(Clone, Debug)]
pub struct MovingBlock {
    pub pos: Pos,
    pub moved: u32,
    pub facing: u8,
    pub extending: bool,
    pub source: bool,
    pub progress: f32,
    pub progress_old: f32,
    // Der Client laesst einen angekommenen Block noch fuenf Ticks stehen, bevor er ihn selbst
    // einsetzt -- meist kommt der Server mit dem fertigen Block zuvor.
    death_ticks: u8,
}

impl MovingBlock {
    fn new(pos: Pos, moved: u32, facing: u8, extending: bool, source: bool) -> Self {
        Self { pos, moved, facing, extending, source, progress: 0.0, progress_old: 0.0, death_ticks: 0 }
    }

    pub fn movement(&self) -> u8 {
        if self.extending {
            self.facing
        } else {
            opposite(self.facing)
        }
    }

    fn extended_progress(&self, p: f32) -> f32 {
        if self.extending {
            p - 1.0
        } else {
            1.0 - p
        }
    }

    // Versatz gegenueber der Zielzelle, mit dem Teilschritt zwischen zwei Ticks.
    pub fn offset(&self, partial: f32) -> DVec3 {
        let p = self.progress_old + (self.progress - self.progress_old) * partial.min(1.0);
        step_vec(self.facing) * self.extended_progress(p) as f64
    }

    fn sticky_piston(&self) -> bool {
        name(self.moved) == "sticky_piston"
    }

    // Was gezeichnet wird: (State, mit Versatz?). Der Kopf ist bis zur Haelfte kurz; ein
    // einfahrender Kolben zeigt seinen Sockel (ausgefahren) fest und den Kopf in Fahrt.
    pub fn render_states(&self, partial: f32) -> Vec<(u32, bool)> {
        let p = self.progress_old + (self.progress - self.progress_old) * partial.min(1.0);
        if name(self.moved) == "piston_head" {
            let kurz = if p <= 0.5 { "true" } else { "false" };
            return state_with(self.moved, "short", kurz).map(|s| vec![(s, true)]).unwrap_or_default();
        }
        if self.source && !self.extending && is_piston_base(self.moved) {
            let facing = direction_of(self.moved).unwrap_or(self.facing);
            let mut out = Vec::new();
            if let Some(kopf) = piston_head(facing, self.sticky_piston(), p >= 0.5) {
                out.push((kopf, true));
            }
            if let Some(sockel) = state_with(self.moved, "extended", "true") {
                out.push((sockel, false));
            }
            return out;
        }
        vec![(self.moved, true)]
    }

    // getCollisionRelatedBlockState: woran sich der Schub misst.
    fn push_state(&self) -> u32 {
        if !self.extending && self.source && is_piston_base(self.moved) {
            let facing = direction_of(self.moved).unwrap_or(self.facing);
            return piston_head(facing, self.sticky_piston(), self.progress > 0.25).unwrap_or(self.moved);
        }
        self.moved
    }

    // getCollisionShape als Weltboxen (s. World::moving_boxes).
    fn collision(&self, shapes: &world::BlockShapes, out: &mut Vec<world::MovingBox>) {
        let zelle = DVec3::new(self.pos.0 as f64, self.pos.1 as f64, self.pos.2 as f64);
        let boxen = |state: u32, versatz: DVec3, noclip: Option<u8>, out: &mut Vec<world::MovingBox>| {
            for b in world::collision_boxes_at(state, shapes, self.pos.1, world::ShapeContext::ABSENT) {
                let min = zelle + versatz + DVec3::new(b.min[0] as f64, b.min[1] as f64, b.min[2] as f64);
                let max = zelle + versatz + DVec3::new(b.max[0] as f64, b.max[1] as f64, b.max[2] as f64);
                out.push(world::MovingBox { min: min.to_array(), max: max.to_array(), noclip });
            }
        };
        // Der Sockel eines einfahrenden Kolbens steht fest und haelt immer.
        if !self.extending && self.source && is_piston_base(self.moved) {
            if let Some(sockel) = state_with(self.moved, "extended", "true") {
                boxen(sockel, DVec3::ZERO, None, out);
            }
        }
        let state = if self.source {
            let kurz = self.extending != (1.0 - self.progress < 0.25);
            piston_head(self.facing, false, kurz)
        } else {
            Some(self.moved)
        };
        let Some(state) = state else { return };
        let versatz = step_vec(self.facing) * self.extended_progress(self.progress) as f64;
        let noclip = (self.progress < 1.0).then_some(self.movement());
        boxen(state, versatz, noclip, out);
    }
}

// Achsenparallele Box in Weltkoordinaten.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Aabb {
    min: DVec3,
    max: DVec3,
}

impl Aabb {
    fn intersects(&self, o: &Aabb) -> bool {
        self.min.x < o.max.x && self.max.x > o.min.x && self.min.y < o.max.y && self.max.y > o.min.y && self.min.z < o.max.z && self.max.z > o.min.z
    }

    fn union(&self, o: &Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }

    fn intersect(&self, o: &Aabb) -> Aabb {
        Aabb { min: self.min.max(o.min), max: self.max.min(o.max) }
    }
}

// PistonMath.getMovementArea: die Scheibe der Dicke `d` vor der Vorderseite der Box.
fn movement_area(b: Aabb, dir: u8, d: f64) -> Aabb {
    let positiv = matches!(dir, 1 | 3 | 5);
    let e = if positiv { d } else { -d };
    let (lo, hi) = (e.min(0.0), e.max(0.0));
    let a = axis(dir);
    let kante = if positiv { b.max[a] } else { b.min[a] };
    let mut out = b;
    out.min[a] = kante + lo;
    out.max[a] = kante + hi;
    out
}

// Wie weit die Box `a` (in Fahrtrichtung `dir`) in die Box `e` hineinragt.
fn overlap(a: &Aabb, dir: u8, e: &Aabb) -> f64 {
    let i = axis(dir);
    if matches!(dir, 1 | 3 | 5) {
        a.max[i] - e.min[i]
    } else {
        e.max[i] - a.min[i]
    }
}

#[derive(Default)]
pub struct Pistons {
    pub moving: Vec<MovingBlock>,
    // Zaehlt die Ticks -- Vanillas Spielzeit fuer die Schub-Grenze je Tick (s. piston_move).
    game_tick: u64,
}

impl Pistons {
    pub fn clear(&mut self, world: &mut World) {
        self.moving.clear();
        world.moving_boxes.clear();
    }

    fn add(&mut self, b: MovingBlock) {
        self.moving.retain(|m| m.pos != b.pos);
        self.moving.push(b);
    }

    // Block-Event an `pos`. false: dort steht gar kein Kolben (dann gehoert das Event jemand
    // anderem, z.B. einer Truhe).
    pub fn block_event(&mut self, world: &mut World, shapes: &world::BlockShapes, pos: Pos, typ: u8, data: u8) -> bool {
        let state = world.block_at(pos.0, pos.1, pos.2);
        if !is_piston_base(state) {
            return false;
        }
        let Some(facing) = direction_of(state) else { return true };
        let sticky = name(state) == "sticky_piston";
        if typ == 0 {
            if self.move_blocks(world, pos, facing, true, sticky) {
                if let Some(aus) = state_with(state, "extended", "true") {
                    world.set_block(pos.0, pos.1, pos.2, aus);
                }
            }
        } else if typ == 1 || typ == 2 {
            let kopf = rel(pos, facing, 1);
            self.final_tick_at(world, kopf);
            if let Some(m) = moving_piston(facing, sticky) {
                world.set_block(pos.0, pos.1, pos.2, m);
            }
            let sockel = state_of(name(state), &[("facing", NAMES[(data & 7).min(5) as usize])]).unwrap_or(state);
            self.add(MovingBlock::new(pos, sockel, facing, false, true));
            if sticky {
                let p2 = rel(pos, facing, 2);
                let s2 = world.block_at(p2.0, p2.1, p2.2);
                let mut erledigt = false;
                if name(s2) == "moving_piston" {
                    if let Some(i) = self.moving.iter().position(|m| m.pos == p2 && m.facing == facing && m.extending) {
                        self.final_tick(world, i);
                        erledigt = true;
                    }
                }
                if !erledigt {
                    let zieht = typ == 1
                        && !is_air(s2)
                        && is_pushable(world, s2, p2, opposite(facing), false, facing)
                        && (reaction(s2) == Reaction::Normal || is_piston_base(s2));
                    if zieht {
                        self.move_blocks(world, pos, facing, false, sticky);
                    } else {
                        world.set_block(kopf.0, kopf.1, kopf.2, air());
                    }
                }
            } else {
                world.set_block(kopf.0, kopf.1, kopf.2, air());
            }
        }
        self.update_collision(world, shapes);
        true
    }

    // PistonBaseBlock.moveBlocks.
    fn move_blocks(&mut self, world: &mut World, pos: Pos, facing: u8, extending: bool, sticky: bool) -> bool {
        let kopf = rel(pos, facing, 1);
        if !extending && name(world.block_at(kopf.0, kopf.1, kopf.2)) == "piston_head" {
            world.set_block(kopf.0, kopf.1, kopf.2, air());
        }
        let (to_push, to_destroy) = {
            let mut r = Resolver::new(world, pos, facing, extending);
            if !r.resolve() {
                return false;
            }
            (r.to_push, r.to_destroy)
        };
        let states: Vec<u32> = to_push.iter().map(|p| world.block_at(p.0, p.1, p.2)).collect();
        // Zellen, die am Ende leer zurueckbleiben (wo nichts Neues hineinfaehrt).
        let mut frei: Vec<Pos> = to_push.clone();
        for p in to_destroy.iter().rev() {
            world.set_block(p.0, p.1, p.2, air());
        }
        let richtung = if extending { facing } else { opposite(facing) };
        for k in (0..to_push.len()).rev() {
            let ziel = rel(to_push[k], richtung, 1);
            frei.retain(|q| *q != ziel);
            if let Some(m) = moving_piston(facing, false) {
                world.set_block(ziel.0, ziel.1, ziel.2, m);
            }
            self.add(MovingBlock::new(ziel, states[k], facing, extending, false));
        }
        if extending {
            if let (Some(kopf_state), Some(m)) = (piston_head(facing, sticky, false), moving_piston(facing, sticky)) {
                frei.retain(|q| *q != kopf);
                world.set_block(kopf.0, kopf.1, kopf.2, m);
                self.add(MovingBlock::new(kopf, kopf_state, facing, true, true));
            }
        }
        for p in frei {
            world.set_block(p.0, p.1, p.2, air());
        }
        true
    }

    fn final_tick_at(&mut self, world: &mut World, pos: Pos) {
        if let Some(i) = self.moving.iter().position(|m| m.pos == pos) {
            self.final_tick(world, i);
        }
    }

    // finalTick: die Fahrt abbrechen und das Ergebnis sofort einsetzen -- der Kopf eines
    // vorzeitig einfahrenden Kolbens verschwindet dabei ganz.
    fn final_tick(&mut self, world: &mut World, i: usize) {
        let m = self.moving.remove(i);
        if name(world.block_at(m.pos.0, m.pos.1, m.pos.2)) == "moving_piston" {
            let neu = if m.source { air() } else { m.moved };
            world.set_block(m.pos.0, m.pos.1, m.pos.2, neu);
        }
    }

    // Der Server hat eine Zelle neu gesetzt: steht dort kein moving_piston mehr, ist die Fahrt
    // dort vorbei (Vanilla entfernt mit dem Block auch sein Block-Entity).
    pub fn block_changed(&mut self, pos: Pos, state: u32) {
        if name(state) != "moving_piston" {
            self.moving.retain(|m| m.pos != pos);
        }
    }

    pub fn update_collision(&self, world: &mut World, shapes: &world::BlockShapes) {
        let mut boxen = Vec::new();
        for m in &self.moving {
            m.collision(shapes, &mut boxen);
        }
        world.moving_boxes = boxen;
    }

    // Ein Client-Tick aller fahrenden Bloecke (nach dem Spieler-Tick, wie Vanillas Block-
    // Entities). `schiebbar`: der eigene Spieler laesst sich schieben (nicht Zuschauer, nicht
    // reitend, nicht tot).
    pub fn tick(&mut self, world: &mut World, shapes: &world::BlockShapes, player: &mut PlayerPhysics, schiebbar: bool) {
        self.game_tick += 1;
        let game_tick = self.game_tick;
        let mut i = 0;
        while i < self.moving.len() {
            let m = &mut self.moving[i];
            m.progress_old = m.progress;
            if m.progress_old >= 1.0 {
                if m.death_ticks < 5 {
                    m.death_ticks += 1;
                    i += 1;
                    continue;
                }
                let m = self.moving.remove(i);
                if name(world.block_at(m.pos.0, m.pos.1, m.pos.2)) == "moving_piston" {
                    let neu = if world::state_props::property(m.moved, "waterlogged") == Some("true") {
                        state_with(m.moved, "waterlogged", "false").unwrap_or(m.moved)
                    } else {
                        m.moved
                    };
                    world.set_block(m.pos.0, m.pos.1, m.pos.2, neu);
                }
                self.update_collision(world, shapes);
                continue;
            }
            let ziel = m.progress + 0.5;
            if schiebbar {
                self.update_collision(world, shapes);
                let m = self.moving[i].clone();
                push_player(&m, ziel, world, shapes, player, game_tick);
                carry_player(&m, ziel, world, shapes, player, game_tick);
            }
            let m = &mut self.moving[i];
            m.progress = ziel.min(1.0);
            i += 1;
        }
        self.update_collision(world, shapes);
    }
}

fn player_box(p: &PlayerPhysics) -> Aabb {
    let halb = p.width() / 2.0;
    Aabb { min: DVec3::new(p.pos.x - halb, p.pos.y, p.pos.z - halb), max: DVec3::new(p.pos.x + halb, p.pos.y + p.current_height(), p.pos.z + halb) }
}

// moveByPositionAndProgress: eine zellrelative Box an die aktuelle Stelle des Blocks.
fn placed(m: &MovingBlock, b: &world::ShapeBox) -> Aabb {
    let lokal = |v: [f32; 3]| DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64);
    placed_raw(m, lokal(b.min), lokal(b.max))
}

fn placed_raw(m: &MovingBlock, min: DVec3, max: DVec3) -> Aabb {
    let d = m.extended_progress(m.progress) as f64;
    let s = step_vec(m.facing);
    let o = DVec3::new(m.pos.0 as f64 + d * s.x, m.pos.1 as f64 + d * s.y, m.pos.2 as f64 + d * s.z);
    Aabb { min: o + min, max: o + max }
}

// moveEntityByPiston: der Schub selbst, waehrend dessen Bloecke, die in `noclip` fahren, nicht
// stoppen.
fn move_by_piston(noclip: u8, player: &mut PlayerPhysics, amount: f64, dir: u8, world: &World, shapes: &world::BlockShapes, game_tick: u64) {
    crate::physics::with_piston_noclip(noclip, || player.piston_move(step_vec(dir) * amount, game_tick, world, shapes));
}

// moveCollidedEntities fuer den eigenen Spieler.
fn push_player(m: &MovingBlock, ziel: f32, world: &World, shapes: &world::BlockShapes, player: &mut PlayerPhysics, game_tick: u64) {
    let dir = m.movement();
    let d = (ziel - m.progress) as f64;
    let boxen = world::collision_boxes_at(m.push_state(), shapes, m.pos.1, world::ShapeContext::ABSENT);
    let Some(erste) = boxen.first() else { return };
    let grenzen = boxen.iter().skip(1).fold(placed(m, erste), |a, b| a.union(&placed(m, b)));
    let bereich = movement_area(grenzen, dir, d).union(&grenzen);
    if !bereich.intersects(&player_box(player)) {
        return;
    }
    // Schleim: die Geschwindigkeit auf der Fahrtachse wird zur Fahrtrichtung -- das Schleudern.
    if name(m.moved) == "slime_block" {
        player.vel[axis(dir)] = step_vec(dir)[axis(dir)];
    }
    let spieler = player_box(player);
    let mut weite = 0.0f64;
    for b in &boxen {
        let flaeche = movement_area(placed(m, b), dir, d);
        if flaeche.intersects(&spieler) {
            weite = weite.max(overlap(&flaeche, dir, &spieler));
            if weite >= d {
                break;
            }
        }
    }
    if weite <= 0.0 {
        return;
    }
    let weite = weite.min(d) + 0.01;
    move_by_piston(dir, player, weite, dir, world, shapes, game_tick);
    if !m.extending && m.source {
        fix_within_base(m.pos, player, dir, d, world, shapes, game_tick);
    }
}

// fixEntityWithinPistonBase: steckt man nach dem Einfahren noch im Sockel, wird man hinaus-
// geschoben.
fn fix_within_base(pos: Pos, player: &mut PlayerPhysics, dir: u8, d: f64, world: &World, shapes: &world::BlockShapes, game_tick: u64) {
    let spieler = player_box(player);
    let zelle = Aabb { min: DVec3::new(pos.0 as f64, pos.1 as f64, pos.2 as f64), max: DVec3::new(pos.0 as f64 + 1.0, pos.1 as f64 + 1.0, pos.2 as f64 + 1.0) };
    if !spieler.intersects(&zelle) {
        return;
    }
    let gegen = opposite(dir);
    let g = overlap(&zelle, gegen, &spieler) + 0.01;
    let h = overlap(&zelle, gegen, &spieler.intersect(&zelle)) + 0.01;
    if (g - h).abs() < 0.01 {
        let g = g.min(d) + 0.01;
        move_by_piston(dir, player, g, gegen, world, shapes, game_tick);
    }
}

// moveStuckEntities: Honig nimmt waagerecht mit, wer auf ihm steht.
fn carry_player(m: &MovingBlock, ziel: f32, world: &World, shapes: &world::BlockShapes, player: &mut PlayerPhysics, game_tick: u64) {
    if name(m.moved) != "honey_block" {
        return;
    }
    let dir = m.movement();
    if axis(dir) == 1 {
        return;
    }
    let oben = world::collision_boxes_at(m.moved, shapes, m.pos.1, world::ShapeContext::ABSENT).iter().map(|b| b.max[1] as f64).fold(0.0, f64::max);
    // Die Obergrenze ist in Vanilla 1.500001, nicht exakt anderthalb.
    let bereich = placed_raw(m, DVec3::new(0.0, oben, 0.0), DVec3::new(1.0, 1.500001, 1.0));
    let d = (ziel - m.progress) as f64;
    let p = player.pos;
    let steht = player.on_ground && p.x >= bereich.min.x && p.x <= bereich.max.x && p.z >= bereich.min.z && p.z <= bereich.max.z;
    if steht && bereich.intersects(&player_box(player)) {
        move_by_piston(dir, player, d, dir, world, shapes, game_tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(block: &str, props: &[(&str, &str)]) -> u32 {
        state_of(block, props).unwrap_or_else(|| panic!("{block} {props:?}"))
    }

    fn welt(boden_y: i32) -> World {
        let stein = st("stone", &[]);
        let mut w = World::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    let base_y = world::MIN_Y + i * 16;
                    if boden_y >= base_y && boden_y < base_y + 16 {
                        let ly = boden_y - base_y;
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((ly * 16 + z) * 16 + x) as usize] = stein;
                            }
                        }
                    }
                    sections.push(world::ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    #[test]
    fn daten_kennen_die_wichtigsten_faelle() {
        assert_eq!(reaction(st("piston", &[])), Reaction::Block);
        assert_eq!(reaction(st("torch", &[])), Reaction::Destroy);
        assert_eq!(reaction(st("white_glazed_terracotta", &[])), Reaction::PushOnly);
        assert_eq!(reaction(st("slime_block", &[])), Reaction::Normal);
        assert!(has_block_entity(st("chest", &[])));
        assert!(!has_block_entity(st("stone", &[])));
    }

    #[test]
    fn kolben_schiebt_schleim_samt_klebenden_nachbarn() {
        let mut w = welt(63);
        let kolben = st("piston", &[("facing", "up")]);
        let schleim = st("slime_block", &[]);
        let erde = st("dirt", &[]);
        w.set_block(0, 64, 0, kolben);
        w.set_block(0, 65, 0, schleim);
        w.set_block(1, 65, 0, erde);
        let mut r = Resolver::new(&w, (0, 64, 0), UP, true);
        assert!(r.resolve());
        assert_eq!(r.to_push.len(), 2, "{:?}", r.to_push);
        assert!(r.to_push.contains(&(0, 65, 0)) && r.to_push.contains(&(1, 65, 0)));
    }

    #[test]
    fn obsidian_blockiert_und_mehr_als_zwoelf_gehen_nicht() {
        let mut w = welt(63);
        w.set_block(0, 64, 0, st("piston", &[("facing", "east")]));
        w.set_block(1, 64, 0, st("dirt", &[]));
        w.set_block(2, 64, 0, st("obsidian", &[]));
        assert!(!Resolver::new(&w, (0, 64, 0), 5, true).resolve());
        let mut w = welt(63);
        w.set_block(0, 64, 0, st("piston", &[("facing", "east")]));
        for x in 1..=13 {
            w.set_block(x, 64, 0, st("dirt", &[]));
        }
        assert!(!Resolver::new(&w, (0, 64, 0), 5, true).resolve());
        w.set_block(13, 64, 0, world::STATE_AIR);
        assert!(Resolver::new(&w, (0, 64, 0), 5, true).resolve());
    }

    #[test]
    fn fackel_davor_zerbricht() {
        let mut w = welt(63);
        w.set_block(0, 64, 0, st("piston", &[("facing", "east")]));
        w.set_block(1, 64, 0, st("dirt", &[]));
        w.set_block(2, 64, 0, st("torch", &[]));
        let mut r = Resolver::new(&w, (0, 64, 0), 5, true);
        assert!(r.resolve());
        assert_eq!(r.to_push, vec![(1, 64, 0)]);
        assert_eq!(r.to_destroy, vec![(2, 64, 0)]);
    }

    #[test]
    fn schleim_schleudert_den_spieler_nach_oben() {
        let mut w = welt(63);
        let shapes = world::BlockShapes::new(Vec::new());
        w.set_block(0, 64, 0, st("piston", &[("facing", "up")]));
        w.set_block(0, 65, 0, st("slime_block", &[]));
        let mut spieler = PlayerPhysics::new(DVec3::new(0.5, 66.0, 0.5));
        spieler.on_ground = true;
        let mut kolben = Pistons::default();
        assert!(kolben.block_event(&mut w, &shapes, (0, 64, 0), 0, UP));
        assert_eq!(name(w.block_at(0, 66, 0)), "moving_piston");
        assert_eq!(name(w.block_at(0, 65, 0)), "moving_piston");
        assert_eq!(w.block_at(0, 64, 0), st("piston", &[("facing", "up"), ("extended", "true")]));
        kolben.tick(&mut w, &shapes, &mut spieler, true);
        assert_eq!(spieler.vel.y, 1.0, "Schleim setzt die Geschwindigkeit auf 1");
        assert!(spieler.pos.y > 66.4, "um den halben Hub (+0.01) geschoben: {}", spieler.pos.y);
        kolben.tick(&mut w, &shapes, &mut spieler, true);
        assert!(kolben.moving.iter().all(|m| m.progress == 1.0));
        for _ in 3..9 {
            kolben.tick(&mut w, &shapes, &mut spieler, false);
        }
        assert!(kolben.moving.is_empty());
        assert_eq!(name(w.block_at(0, 66, 0)), "slime_block");
        assert_eq!(name(w.block_at(0, 65, 0)), "piston_head");
    }

    #[test]
    fn schub_je_tick_hoechstens_null_komma_fuenf_eins() {
        let w = welt(63);
        let shapes = world::BlockShapes::new(Vec::new());
        let mut spieler = PlayerPhysics::new(DVec3::new(0.5, 70.0, 0.5));
        spieler.piston_move(DVec3::new(0.4, 0.0, 0.0), 7, &w, &shapes);
        spieler.piston_move(DVec3::new(0.4, 0.0, 0.0), 7, &w, &shapes);
        assert!((spieler.pos.x - 1.01).abs() < 1e-9, "{}", spieler.pos.x);
        spieler.piston_move(DVec3::new(0.4, 0.0, 0.0), 8, &w, &shapes);
        assert!((spieler.pos.x - 1.41).abs() < 1e-9, "neuer Tick, neues Budget: {}", spieler.pos.x);
    }
}
