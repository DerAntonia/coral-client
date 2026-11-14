// Entity-Culling (Settings::entity_culling), Stufe 1: was Vanilla schon aussortiert und dieser
// Client bisher nicht. Ein Entity wird nur gebaut, wenn seine Box (um 0.5 vergroessert) im
// Sichtkegel liegt und seine Section nach dem Hoehlen-Culling erreichbar ist
// (LevelRenderer.extractVisibleEntities: EntityRenderer.shouldRender + isSectionCompiledAndVisible).
// Blockentities nur aus Sections, die im Sichtkegel liegen und erreichbar sind
// (extractVisibleBlockEntities laeuft ueber visibleSections).
//
// Stufe 2 (Settings::entity_ray_culling): zusaetzlich Sichtlinien wie die Mod EntityCulling --
// von der Kamera aus Strahlen durchs Blockraster (s. render::sight). Gefragt wird jeden Frame
// fuer ALLE Entities in Reichweite, nicht nur die im Bild: dreht man sich um, liegt das
// Ergebnis schon vor.
//
// Bewusst NIE weg: Namensschilder (eigener Weg, build_world_labels), der eigene Spieler (eigener
// Zweig), das eigene Fahrzeug. Leuchtende verlieren nur den Sichtkegel-Test, nicht den
// Hoehlen- oder Sichtlinien-Test: ihr Umriss ist durch Waende zu sehen.

use super::*;

impl App {
    // Zu Beginn jedes Weltframes: der Sichtkegel, gegen den dieser Frame prueft. Augenrelativ
    // wie die Geometrie (s. GpuState::render_world).
    pub(crate) fn update_cull_frustum(&mut self) {
        let kegel = self.gpu.as_ref().filter(|_| self.settings.entity_culling).map(|gpu| {
            let aspect = gpu.config.width as f32 / gpu.config.height.max(1) as f32;
            render::Frustum::from_view_proj(self.camera.cull_view_proj_matrix(aspect))
        });
        self.cull_frustum.set(kegel);
        self.query_sight();
    }

    // Stufe 2: Ergebnis abholen und, wenn der Thread frei ist, die naechste Anfrage stellen.
    fn query_sight(&mut self) {
        let viewer = self.camera.view_position();
        let an = self.settings.entity_culling && self.settings.entity_ray_culling;
        let Some(mut liste) = self.gpu.as_mut().and_then(|g| g.block_renderer.sight.begin_frame(viewer)) else { return };
        if !an {
            return;
        }
        let progress = self.tick_progress();
        for (&id, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) || self.cull_exempt(id, e) {
                continue;
            }
            // Fuer die Strahlen die echte Trefferbox, NICHT die vergroesserte: deren Rand laege
            // bei einem Entity an der Wand in der Wand, und die Zielzelle zaehlt nicht mit.
            let (x, y, z) = e.render_pos(progress);
            let (w, h) = e.hit_size();
            let (min, max) = if w <= 0.0 || h <= 0.0 {
                entity_cull_box(e, (x, y, z))
            } else {
                (DVec3::new(x - w / 2.0, y, z - w / 2.0), DVec3::new(x + w / 2.0, y + h, z + w / 2.0))
            };
            liste.push((render::sight::SightTarget::Entity(id), min, max));
        }
        // Blockentities: ihr Block (aus demselben Grund ohne Rand).
        let block = |pos: (i32, i32, i32)| {
            let min = DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
            (render::sight::SightTarget::Block(pos.0, pos.1, pos.2), min, min + DVec3::ONE)
        };
        for (pos, _) in self.world.model_blocks() {
            if self.block_column_drawn(pos) {
                liste.push(block(pos));
            }
        }
        for &pos in self.signs.keys() {
            if self.block_column_drawn(pos) {
                liste.push(block(pos));
            }
        }
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.block_renderer.sight.query(viewer, liste);
        }
    }

    pub(crate) fn sight_hidden(&self, ziel: render::sight::SightTarget) -> bool {
        self.settings.entity_ray_culling && self.gpu.as_ref().is_some_and(|g| g.block_renderer.sight.hidden(ziel))
    }

    // Nie wegschneiden: das eigene Fahrzeug und Leuchtende (Umriss durch Waende).
    fn cull_exempt(&self, id: i32, e: &world::Entity) -> bool {
        e.glowing || self.own_ride.as_ref().is_some_and(|r| r.vehicle == id) || no_frustum_test(e)
    }

    // Section einer Weltposition erreichbar? Ausserhalb der Bauhoehe immer (wie Vanilla).
    fn section_reachable_at(&self, x: i32, y: i32, z: i32) -> bool {
        let Some(gpu) = self.gpu.as_ref() else { return true };
        let index = (y - self.world.min_y()).div_euclid(16);
        gpu.block_renderer.section_reachable((x.div_euclid(16), z.div_euclid(16)), index)
    }

    fn box_in_frustum(&self, kegel: &render::Frustum, min: DVec3, max: DVec3) -> bool {
        let eye = self.camera.position;
        kegel.intersects_aabb((min - eye).as_vec3(), (max - eye).as_vec3())
    }

    // true = dieses Entity in diesem Frame nicht bauen.
    pub(crate) fn entity_culled(&self, id: i32, e: &world::Entity, (x, y, z): (f64, f64, f64)) -> bool {
        let Some(kegel) = self.cull_frustum.get() else { return false };
        if self.own_ride.as_ref().is_some_and(|r| r.vehicle == id) {
            return false;
        }
        if !no_frustum_test(e) {
            let (min, max) = entity_cull_box(e, (x, y, z));
            if !self.box_in_frustum(&kegel, min, max) {
                return true;
            }
        }
        if e.glowing {
            return false;
        }
        !self.section_reachable_at(x.floor() as i32, y.floor() as i32, z.floor() as i32) || self.sight_hidden(render::sight::SightTarget::Entity(id))
    }

    // true = Blockentity an dieser Stelle in diesem Frame nicht bauen: seine Section liegt
    // ausserhalb des Sichtkegels oder ist nicht erreichbar. Ein Block Rand, weil Truhendeckel,
    // Betten und Schilder ueber ihre Zelle hinausreichen.
    pub(crate) fn block_entity_culled(&self, pos: (i32, i32, i32)) -> bool {
        self.cull_frustum.get().is_some() && (self.block_section_culled(pos) || self.sight_hidden(render::sight::SightTarget::Block(pos.0, pos.1, pos.2)))
    }

    // Der Teil von block_entity_culled, der nur an der Section haengt (Sichtkegel und
    // Hoehlen-Culling) -- fuer alle Bloecke einer Section im selben Frame derselbe.
    pub(crate) fn block_section_culled(&self, pos: (i32, i32, i32)) -> bool {
        let Some(kegel) = self.cull_frustum.get() else { return false };
        let (sx, sz) = (pos.0.div_euclid(16) * 16, pos.2.div_euclid(16) * 16);
        let sy = self.world.min_y() + (pos.1 - self.world.min_y()).div_euclid(16) * 16;
        let min = DVec3::new(f64::from(sx) - 1.0, f64::from(sy) - 1.0, f64::from(sz) - 1.0);
        let max = min + DVec3::splat(18.0);
        !self.box_in_frustum(&kegel, min, max) || !self.section_reachable_at(pos.0, pos.1, pos.2)
    }
}

// Ohne Sichtkegel-Test, wie in Vanilla (affectedByCulling false bzw. Display ohne Groesse): sie
// zeichnen weit ueber ihre Box hinaus.
fn no_frustum_test(e: &world::Entity) -> bool {
    matches!(
        world::entity_type_name(e.entity_type),
        Some("ender_dragon" | "lightning_bolt" | "fishing_bobber" | "block_display" | "item_display" | "text_display")
    )
}

// Die Box, gegen die geprueft wird: Trefferbox um 0.5 vergroessert (EntityRenderer.shouldRender),
// ohne Groesse ein Wuerfel von 4 Bloecken um die Fuesse.
fn entity_cull_box(e: &world::Entity, (x, y, z): (f64, f64, f64)) -> (DVec3, DVec3) {
    let (w, h) = e.hit_size();
    if w <= 0.0 || h <= 0.0 {
        return (DVec3::new(x - 2.0, y - 2.0, z - 2.0), DVec3::new(x + 2.0, y + 2.0, z + 2.0));
    }
    let r = w / 2.0 + 0.5;
    (DVec3::new(x - r, y - 0.5, z - r), DVec3::new(x + r, y + h + 0.5, z + r))
}
