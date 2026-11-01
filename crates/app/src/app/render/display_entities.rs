// Glint, Display-Entities, Wegschieben, Sichtweite und Licht.

use super::*;

impl App {
    // Glint-Twins fuer das, was seit `start` in `out` gelandet ist (s. render::glint).
    // `lokal` gibt DIESELBE Geometrie noch einmal mit lokalen UVs aus. Stimmt die Anzahl nicht
    // (ein Modell fehlte beim zweiten Mal), lieber kein Glint als ein verschobener.
    pub(crate) fn push_glint(
        &self,
        out: &[world::BlockVertex],
        start: usize,
        scale: f32,
        texture: f32,
        lokal: impl FnOnce(&mut Vec<world::BlockVertex>),
    ) {
        let mut l = Vec::new();
        lokal(&mut l);
        if l.len() != out.len() - start {
            return;
        }
        render::glint::glint_vertices(&out[start..], &l, scale, texture, &mut self.glint_out.borrow_mut());
    }

    // Ein item_display oder block_display als freistehende Weltgeometrie. `feet` ist die
    // augenrelative Entity-Position; darauf dieselbe Kette wie beim Text-Display
    // (DisplayEntityRenderer.render): Billboard-Drehung, dann die AffineTransformation.
    // Ein item_display dreht danach noch um 180 Grad um Y (ItemDisplayEntityRenderer).
    //
    // text_display kommt hier NICHT vor, s. build_display_text.
    pub(crate) fn emit_display_entity(&self, display: &world::DisplayInfo, age: f32, feet: [f32; 3], yaw: f32, pitch: f32, out: &mut Vec<world::BlockVertex>) {
        // block_display laeuft ueber den Block-Atlas, s. display_block_vertices.
        if matches!(display.content, world::DisplayContent::Block(_)) {
            return;
        }
        let Some((item, _)) = Self::display_model(display) else { return };
        let m = self.display_matrix(display, age, feet, yaw, pitch) * glam::Mat4::from_rotation_y(std::f32::consts::PI);
        self.held_items.set_skull(self.held_skull_texture(&item));
        self.held_items.emit_display(item.model_id, item.potion_color, affine_from_mat4(m), display.item_mode, ENTITY_BRIGHTNESS, out);
    }

    // DisplayEntityRenderer.render: Entity-Position, Billboard-Drehung, AffineTransformation
    // (zum Entity-Alter `age` interpoliert, s. world::DisplayInfo::transform_at).
    pub(crate) fn display_matrix(&self, display: &world::DisplayInfo, age: f32, feet: [f32; 3], yaw: f32, pitch: f32) -> glam::Mat4 {
        let (cam_yaw, cam_pitch) = self.vanilla_camera_angles();
        glam::Mat4::from_translation(glam::Vec3::from(feet))
            * display_billboard_matrix(display.billboard, yaw, pitch, cam_yaw, cam_pitch)
            * display_affine_matrix(&display.transform_at(age))
    }

    // Alle block_displays als echte Blockgeometrie im Block-Atlas (augenrelativ, s.
    // world::mesher::block_display_vertices). Sie reisen im Puffer der Geisterbloecke mit, in
    // dessen deckendem Teil -- derselbe Atlas, dieselbe Pipeline, und der wird ohnehin jedes
    // Bild neu gebaut.
    pub(crate) fn display_block_vertices(&self, out: &mut Vec<world::mesher::BlockVertex>) {
        let eye = self.camera.position;
        let progress = self.tick_progress();
        for (_, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) {
                continue;
            }
            let Some(display) = &e.display else { continue };
            let world::DisplayContent::Block(state) = display.content else { continue };
            if state == world::STATE_AIR {
                continue;
            }
            let (x, y, z) = e.render_pos(progress);
            let v = (DVec3::new(x, y, z) - eye).as_vec3();
            let m = self.display_matrix(display, e.age_ticks(progress), [v.x, v.y, v.z], e.render_yaw(progress), e.render_pitch(progress));
            let start = out.len();
            out.extend(world::mesher::block_display_vertices(state, &self.block_uvs, &self.block_shapes, &self.cross_blocks));
            for vert in &mut out[start..] {
                vert.position = m.transform_point3(glam::Vec3::from(vert.position)).into();
            }
        }
        // Fallende Bloecke (FallingBlockRenderer): das Blockmodell um einen halben Block nach
        // -x/-z versetzt, denn das Entity steht in der Mitte der Zelle. Steht an seiner Stelle
        // schon derselbe Block (gerade gelandet, das Entfernen ist noch unterwegs), wird er
        // nicht doppelt gezeichnet -- so macht es auch Vanilla.
        for (_, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) {
                continue;
            }
            let Some(state) = e.falling_block else { continue };
            if state == world::STATE_AIR {
                continue;
            }
            let (x, y, z) = e.render_pos(progress);
            if self.world.block_at(x.floor() as i32, y.floor() as i32, z.floor() as i32) == state {
                continue;
            }
            let v = (DVec3::new(x - 0.5, y, z - 0.5) - eye).as_vec3();
            let start = out.len();
            out.extend(world::mesher::block_display_vertices(state, &self.block_uvs, &self.block_shapes, &self.cross_blocks));
            for vert in &mut out[start..] {
                vert.position = [vert.position[0] + v.x, vert.position[1] + v.y, vert.position[2] + v.z];
            }
        }
        // Gezuendetes TNT (TntRenderer): der Block um seine Mitte, eine halbe Hoehe ueber den
        // Fuessen. In den letzten zehn Ticks blaeht er sich bis auf 130 % auf, und alle fuenf
        // Ticks blitzt er weiss (TntMinecartRenderer.renderWhiteSolidBlock).
        let tnt_vorgabe = block_place::default_state_for_item("tnt");
        for (_, e) in self.entities.iter() {
            if world::entity_type_name(e.entity_type) != Some("tnt") || !self.entity_in_render_distance(e) {
                continue;
            }
            let Some(state) = e.tnt_block.or(tnt_vorgabe).filter(|s| *s != world::STATE_AIR) else { continue };
            let rest = e.tnt_fuse as f32 - progress + 1.0;
            let groesse = if rest < 10.0 {
                let f = (1.0 - rest / 10.0).clamp(0.0, 1.0);
                1.0 + f * f * f * f * 0.3
            } else {
                1.0
            };
            let blitz = (e.tnt_fuse / 5) % 2 == 0;
            let (x, y, z) = e.render_pos(progress);
            let mitte = (DVec3::new(x, y + 0.5, z) - eye).as_vec3();
            let start = out.len();
            out.extend(world::mesher::block_display_vertices(state, &self.block_uvs, &self.block_shapes, &self.cross_blocks));
            for vert in &mut out[start..] {
                let p = (glam::Vec3::from(vert.position) - glam::Vec3::splat(0.5)) * groesse + mitte;
                vert.position = p.into();
                if blitz {
                    *vert = vert.with_white_flash();
                }
            }
        }
        // Vom Kolben bewegte Bloecke (PistonHeadRenderer): ihre Zelle zeigt moving_piston, also
        // nichts; der Block selbst faehrt hier zwischen Start- und Zielzelle.
        for m in &self.pistons.moving {
            let versatz = m.offset(progress);
            for (state, faehrt) in m.render_states(progress) {
                let zelle = DVec3::new(f64::from(m.pos.0), f64::from(m.pos.1), f64::from(m.pos.2));
                let v = (zelle + if faehrt { versatz } else { DVec3::ZERO } - eye).as_vec3();
                let start = out.len();
                out.extend(world::mesher::block_display_vertices(state, &self.block_uvs, &self.block_shapes, &self.cross_blocks));
                for vert in &mut out[start..] {
                    vert.position = [vert.position[0] + v.x, vert.position[1] + v.y, vert.position[2] + v.z];
                }
            }
        }
    }

    // (Item-Id, mittig zentriert?) fuer den Inhalt eines Display-Entities.
    //
    // Ein block_display traegt einen BLOCK-State, die Texturen dieses Clients haengen aber am
    // ITEM (s. held_item::HeldItems) -- deshalb der Umweg ueber den Blocknamen. Das ist eine
    // bewusste Naeherung mit bekannter Grenze: gezeichnet wird der Wuerfel des zugehoerigen
    // Item-Modells, nicht die echte Blockform. Ein block_display mit einer Treppe erscheint
    // damit als voller Wuerfel. Der Alternativweg (echte Blockgeometrie aus dem Mesher) liegt
    // im BLOCK-Atlas und waere ein zusaetzlicher Zeichenaufruf pro Display mitten in der
    // Entity-Schleife.
    pub(crate) fn display_model(display: &world::DisplayInfo) -> Option<(world::HeldItem, bool)> {
        match &display.content {
            world::DisplayContent::Item(item) => Some(((*item)?, true)),
            world::DisplayContent::Block(state) => {
                let name = world::block_registry::block_name(*state)?;
                // Luft ist der Vorgabewert eines frisch gespawnten block_display -- und
                // zeichnet nichts.
                if name == "air" {
                    return None;
                }
                Some((world::HeldItem::plain(world::item_registry::item_id_by_name(name)?), false))
            }
            world::DisplayContent::Text(_) => None,
        }
    }

    // Das Schubsen durch andere Lebewesen (Dorfbewohner, Mobs, Spieler), wie es der Vanilla-
    // Client selbst rechnet: jedes Lebewesen fragt in seinem Tick (LivingEntity.tickCramming),
    // ob seine Hitbox die des lokalen Spielers schneidet (ClientWorld.getCrammedEntities
    // liefert dort nur ihn), und stoesst ihn dann weg (Entity.pushAwayFrom): waagerecht vom
    // Schieber weg, je Achse hoechstens 0.05, schwaecher nur auf sehr kurze Entfernung nicht.
    // Vanilla tickt den Spieler vor den Mobs; ihr Stoss landet also in der Geschwindigkeit des
    // NAECHSTEN Spieler-Ticks -- deshalb wird er hier direkt davor addiert.
    //
    // Fehlte ganz: zwischen Dorfbewohnern lief die Grim-Simulation laufend auseinander (Fehler
    // ~0.04, der mit der Bodenreibung 0.546 je Tick auslief -- genau ein fehlender Stoss).
    pub(crate) fn entity_push(&self) -> DVec3 {
        let p = self.physics.pos;
        // LivingEntity.isPushable fuer den Spieler: lebendig, kein Zuschauer, klettert nicht.
        if self.is_dead || self.game_mode == GAME_MODE_SPECTATOR || self.physics.is_climbing(&self.world) {
            return DVec3::ZERO;
        }
        let halb = self.physics.width() / 2.0;
        let (p_min, p_max) = (DVec3::new(p.x - halb, p.y, p.z - halb), DVec3::new(p.x + halb, p.y + self.physics.current_height(), p.z + halb));
        let eigenes_team = self.own_uuid.and_then(|u| self.player_names.get(&u)).and_then(|n| self.sb_entry_team.get(n)).map(String::as_str);
        let regel = |team: Option<&str>| team.and_then(|t| self.sb_teams.get(t)).map_or(0, |t| t.collision_rule);
        let mut schub = DVec3::ZERO;
        for (_, e) in self.entities.iter() {
            if !world::is_living(e.entity_type) || world::entity_type_name(e.entity_type) == Some("armor_stand") {
                continue;
            }
            let (x, y, z) = e.pos;
            let (w, h) = e.box_size();
            let schneidet = x - w / 2.0 < p_max.x && x + w / 2.0 > p_min.x && y < p_max.y && y + h > p_min.y && z - w / 2.0 < p_max.z && z + w / 2.0 > p_min.z;
            if !schneidet {
                continue;
            }
            let eintrag = if world::is_player(e.entity_type) { self.player_names.get(&e.uuid).cloned() } else { Some(uuid_hyphenated(&e.uuid)) };
            let team = eintrag.as_ref().and_then(|n| self.sb_entry_team.get(n)).map(String::as_str);
            if !team_allows_push(team, regel(team), eigenes_team, regel(eigenes_team)) {
                continue;
            }
            let (mut dx, mut dz) = (x - p.x, z - p.z);
            let mut f = dx.abs().max(dz.abs());
            if f < f64::from(0.01f32) {
                continue;
            }
            f = f.sqrt();
            dx /= f;
            dz /= f;
            let g = (1.0 / f).min(1.0);
            // Dieselbe Reihenfolge wie Vanilla: erst g, dann der Faktor (Rundung).
            dx *= g;
            dz *= g;
            dx *= f64::from(0.05f32);
            dz *= f64::from(0.05f32);
            schub.x -= dx;
            schub.z -= dz;
        }
        schub
    }

    // Liegt das Entity innerhalb der Entity-Renderdistanz (Settings::entity_render_distance)?
    // Gemessen von der Kamera aus; gilt fuer alles Gezeichnete, nicht fuers Anvisieren.
    pub(crate) fn entity_in_render_distance(&self, e: &world::Entity) -> bool {
        // Durch wessen Augen man sieht, das zeichnet Vanilla in der Ich-Sicht nicht.
        if self.perspective.is_first_person() && self.spectated_entity().is_some_and(|c| std::ptr::eq(c, e)) {
            return false;
        }
        let (x, y, z) = e.pos;
        // Wie Vanilla nur in gezeichneten Chunks (LevelExtractor.isEntityVisible ->
        // LevelRenderer.isSectionCompiledAndVisible; ausserhalb der Bauhoehe immer). Beim
        // Entladen eines Chunks wirft Vanilla dessen Entities nicht weg, es tickt und zeichnet
        // sie nur nicht mehr. Hier wurden sie weiter gezeichnet, eingefroren an ihrer letzten
        // Stelle, wie Geister-Entities.
        let by = y.floor() as i32;
        let min_y = self.world.min_y();
        if by >= min_y && by < min_y + self.world_height {
            let spalte = ((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16));
            if !self.render_limit_meshed.contains(&spalte) {
                return false;
            }
        }
        let d = self.settings.entity_render_distance;
        if d >= ENTITY_DISTANCE_MAX {
            return true;
        }
        DVec3::new(x, y, z).distance_squared(self.camera.position) <= f64::from(d) * f64::from(d)
    }

    // Shader-Pfad: Licht an einer Blockposition -- Himmelslicht vom Server, Blocklicht aus dem
    // letzten Mesh der Spalte (s. column_light). (Blocklicht je Kanal, Himmelslicht.)
    pub(crate) fn raw_light_at(&self, x: i32, y: i32, z: i32) -> ([u8; 3], u8) {
        let key = (x.div_euclid(16), z.div_euclid(16));
        let (lx, lz) = (x.rem_euclid(16), z.rem_euclid(16));
        let himmel = self.world.column(key.0, key.1).map_or(15, |c| c.sky_at(lx, y, lz));
        let block = self.column_light.get(&key).map_or([0; 3], |l| l.at(lx, y, lz));
        (block, himmel)
    }

    // Licht an einem Punkt, gepackt wie world::light::pack_vertex_light: je Kanal das hellste
    // der (bis zu acht) Felder, die der Punkt beruehrt, undurchsichtige ausgenommen. So
    // bekommt eine Ecke auf der Blockgrenze (Bett, Truhe, Auge an der Decke) das Licht der
    // freien Seite statt der Dunkelheit im Nachbarblock.
    pub(crate) fn light_near(&self, p: DVec3) -> u32 {
        let mut best: Option<([u8; 3], u8)> = None;
        for i in 0..8 {
            let o = |b: u32| if i >> b & 1 == 1 { 0.02 } else { -0.02 };
            let (x, y, z) = ((p.x + o(0)).floor() as i32, (p.y + o(1)).floor() as i32, (p.z + o(2)).floor() as i32);
            if world::light::opacity(self.world.block_at(x, y, z)) >= 15 {
                continue;
            }
            let (b, h) = self.raw_light_at(x, y, z);
            best = Some(match best {
                None => (b, h),
                Some((bb, hh)) => ([bb[0].max(b[0]), bb[1].max(b[1]), bb[2].max(b[2])], hh.max(h)),
            });
        }
        let (b, h) = best.unwrap_or_else(|| self.raw_light_at(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32));
        world::light::pack_vertex_light(b.map(f32::from), f32::from(h))
    }

    // Licht fuer die Vanilla-Vierecke (s. HeldItems::emit_vanilla): je Viereck an seiner Mitte,
    // als UV2 wie Vanillas LightmapTextureManager.pack (Blocklicht * 16, Himmelslicht * 16).
    pub(crate) fn vanilla_item_lights(&self, mut verts: Vec<render::vanilla_draw::VanillaVertex>) -> Vec<render::vanilla_draw::VanillaVertex> {
        let eye = self.camera.position;
        for quad in verts.chunks_mut(4) {
            let mitte = quad.iter().fold(glam::Vec3::ZERO, |a, v| a + glam::Vec3::from(v.position)) / quad.len() as f32;
            let p = eye + mitte.as_dvec3();
            let (block, himmel) = self.raw_light_at(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let b = i32::from(block.iter().copied().max().unwrap_or(0).min(15));
            for v in quad {
                if v.uv2 == [-1, -1] {
                    v.uv2 = [b * 16, i32::from(himmel.min(15)) * 16];
                }
            }
        }
        verts
    }
}
