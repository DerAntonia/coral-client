// Betten, Shulkerkisten, Koepfe, Statuen, Truhen, Schilder und Geschosse.

use super::*;

// Motion-Blur-Schluessel eines Block-Entities: kein Entity, also aus der Zelle (wie die
// Kolbenbloecke in display_block_vertices).
fn block_entity_key(pos: (i32, i32, i32)) -> u64 {
    let (x, y, z) = pos;
    crate::entity_motion::key(crate::entity_motion::KIND_BLOCK_ENTITY, x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ z.wrapping_mul(83_492_791))
}

impl App {
    // Ein Block-Entity-Modell: per GPU-Skinning (s. skin_frame), sonst als Ecken in `out`.
    // Block-Entities werden nie gefaerbt und haben keinen Umriss.
    #[allow(clippy::too_many_arguments)]
    fn emit_block_entity_model(&self, skinning: bool, pos: (i32, i32, i32), model: &world::Model, pose: &world::Pose, base: world::Affine, tex: [f32; 4], out: &mut Vec<world::BlockVertex>) {
        if skinning {
            self.push_skinned(model, pose, base, tex, [ENTITY_BRIGHTNESS; 3], false, block_entity_key(pos));
        } else {
            world::emit_entity_model(model, pose, base, tex, ENTITY_BRIGHTNESS, out);
        }
    }

    // Betten. Sie sind BLOECKE, werden hier aber wie Entities gezeichnet -- ihr Blockmodell ist
    // leer (models/block/bed.json hat nur ein "particle"-Feld), Vanilla zeichnet sie mit einem
    // eigenen BlockEntity-Renderer und einer Entity-Textur. Ohne das bleibt ein Bett komplett
    // unsichtbar.
    //
    // Gezeichnet wird nur, was in der Naehe liegt (s. block_entity_shown) -- auch, damit eine
    // weit geladene Welt nicht tausende Betten pro Frame anfasst.
    // Die Positionen kommen aus World::beds, dem Index, der beim Laden/Aendern gepflegt wird.
    // `eye`: der Bezugspunkt des Entity-Puffers -- alles darin ist augenrelativ (s.
    // build_entity_vertices), die Blockposition wird also davon abgezogen.
    pub(crate) fn emit_beds(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        let skinning = self.skinning_active();
        for (pos, state) in self.world.model_blocks() {
            if !self.block_entity_shown(pos) {
                continue;
            }
            let Some(bett) = world::bed_instance(pos, state) else { continue };
            let tex = self.entity_atlas.rect_or_missing(bett.texture);
            let model = if bett.head { &self.bed_head_model } else { &self.bed_foot_model };
            // isFoot ist im WELT-Renderer immer false: der -1-Versatz gehoert allein zum
            // Item-Modell (renderAsItem zeichnet beide Haelften aus EINEM Ursprung). In der
            // Welt hat jede Haelfte ihr eigenes BlockEntity und damit ihren eigenen Ursprung.
            let base = world::entity_model::bed_transform(block_rel(pos, eye), bett.facing_deg, false);
            self.emit_block_entity_model(skinning, pos, model, &world::Pose::new(), base, tex, out);
        }
    }

    // Shulkerkisten (ShulkerBoxBlockEntityRenderer) -- derselbe Weg wie die Betten, aus
    // demselben Index (World::beds fuehrt beide). Der Deckel folgt block_lids.
    pub(crate) fn emit_shulker_boxes(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        let t = self.tick_progress();
        let model = world::entity_model::shulker_box_model();
        let skinning = self.skinning_active();
        for (pos, state) in self.world.model_blocks() {
            let Some(kiste) = world::shulker_instance(pos, state) else { continue };
            if !self.block_entity_shown(pos) {
                continue;
            }
            let tex = self.entity_atlas.rect_or_missing(kiste.texture);
            let offen = self.block_lids.get(&pos).map_or(0.0, |d| d.last + (d.progress - d.last) * t);
            let base = world::entity_model::shulker_box_transform(block_rel(pos, eye), kiste.facing);
            self.emit_block_entity_model(skinning, pos, model, &world::entity_model::shulker_box_pose(offen), base, tex, out);
        }
    }

    // CORAL_TITLE_DEBUG=1: jeden Titel/Untertitel Abschnitt fuer Abschnitt mit Zeichencodes
    // ausgeben, und welche Zeichen die aktuelle Schrift gar nicht kennt (die fallen beim
    // Zeichnen still weg). Gebaut fuer "beim Titel fehlt oft das letzte Zeichen".
    pub(crate) fn title_debug(&self, art: &str, spans: &[protocol::nbt::TextSpan]) {
        if std::env::var_os("CORAL_TITLE_DEBUG").is_none() {
            return;
        }
        eprintln!("[title] {art}: {} Abschnitt(e)", spans.len());
        for (i, span) in spans.iter().enumerate() {
            let codes: Vec<String> = span
                .text
                .chars()
                .map(|c| {
                    let fehlt = self.gpu.as_ref().is_some_and(|g| g.font_atlas.glyph(c).is_none());
                    format!("U+{:04X}{}", c as u32, if fehlt { "(fehlt)" } else { "" })
                })
                .collect();
            eprintln!("[title]   #{i} farbe={:?} {:?} [{}]", span.color, span.text, codes.join(" "));
        }
    }

    // Profil eines platzierten Kopfes merken: die Textur-Property wie bei Kopf-Items (s.
    // note_head_profile), geladen wird ueber request_head_skins.
    // Block-Entities (Truhen, Betten, Schilder, ...) wie Vanilla nur in gezeichneten Chunks
    // (BlockEntityRenderDispatcher laeuft ueber die kompilierten Sections). Sonst blieben sie
    // nach dem Verkleinern der Sichtweite einzeln in der Luft stehen.
    pub(crate) fn block_column_drawn(&self, pos: (i32, i32, i32)) -> bool {
        self.render_limit_meshed.contains(&(pos.0.div_euclid(16), pos.2.div_euclid(16)))
    }

    // Gezeichneter Chunk und innerhalb der Block-Entity-Renderdistanz (vom Auge aus).
    pub(crate) fn block_entity_shown(&self, pos: (i32, i32, i32)) -> bool {
        let d = self.settings.block_entity_render_distance;
        self.block_column_drawn(pos) && (d >= BLOCK_ENTITY_DISTANCE_MAX || self.near_eye(pos, f64::from(d))) && !self.block_entity_culled(pos)
    }

    pub(crate) fn near_eye(&self, pos: (i32, i32, i32), abstand: f64) -> bool {
        let mitte = DVec3::new(f64::from(pos.0) + 0.5, f64::from(pos.1) + 0.5, f64::from(pos.2) + 0.5);
        (mitte - self.physics.eye_pos()).length_squared() <= abstand * abstand
    }

    pub(crate) fn note_placed_head(&mut self, pos: (i32, i32, i32), profil: &protocol::item::HeadProfile) {
        self.note_head_profile(profil);
        match head_skin_key(profil) {
            Some(key) => {
                self.placed_heads.insert(pos, key);
            }
            None => {
                self.placed_heads.remove(&pos);
            }
        }
    }

    // Platzierte Koepfe (SkullBlockEntityRenderer.render): am Boden auf der Blockmitte, an der
    // Wand ein Viertel Block von der Wand weg und 4 Pixel hoch, gedreht in 22.5-Grad-Stufen.
    pub(crate) fn emit_skulls(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        let skinning = self.skinning_active();
        for (pos, state) in self.world.model_blocks() {
            let Some(kopf) = world::skull_instance(pos, state) else { continue };
            if !self.block_entity_shown(pos) {
                continue;
            }
            let Some((tex, hut)) = self.skull_texture_for(kopf.item, self.placed_heads.get(&pos).copied()) else { continue };
            let b = block_rel(pos, eye);
            let versatz = match kopf.wall {
                None => [0.5, 0.0, 0.5],
                Some((dx, dz)) => [0.5 - dx as f32 * 0.25, 0.25, 0.5 - dz as f32 * 0.25],
            };
            let base = world::Affine::translation([b[0] + versatz[0], b[1] + versatz[1], b[2] + versatz[2]]);
            let base = held_item::skull_transform(base, kopf.yaw_deg);
            self.emit_block_entity_model(skinning, pos, held_item::skull_model(hut), &world::Pose::new(), base, tex, out);
        }
    }

    // Kupfergolem-Statuen (CopperGolemStatueBlockRenderer): das Golem-Modell der Haltung auf
    // der Blockmitte, in die FACING-Richtung gedreht, mit Vanillas Statuen-Haltung (Wurzel auf
    // y = 0, um Z halb gedreht). Frueher als Lebewesen eingebettet -- das stimmte nur fuer
    // den Stehend-Layer, die anderen Haltungen schwebten 1,5 Bloecke hoch.
    pub(crate) fn emit_statues(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        let skinning = self.skinning_active();
        for (pos, state) in self.world.model_blocks() {
            let Some(statue) = world::statue_instance(pos, state) else { continue };
            if !self.block_entity_shown(pos) {
                continue;
            }
            let Some(model) = world::model_data::layer_model(statue.layer) else { continue };
            let tex = self.entity_atlas.rect_or_missing(statue.texture);
            let b = block_rel(pos, eye);
            let base = world::entity_model::copper_golem_statue_transform(b, statue.yaw_deg);
            self.emit_block_entity_model(skinning, pos, model, &world::entity_model::copper_golem_statue_pose(model), base, tex, out);
        }
    }

    // Truhen (ChestBlockEntityRenderer) aus demselben Index wie Betten und Shulker. Eine
    // Doppeltruhe geht als Ganzes auf: Vanilla nimmt den groesseren Deckelstand beider Haelften
    // (ChestBlock.getAnimationProgressRetriever).
    pub(crate) fn emit_chests(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        let t = self.tick_progress();
        let weihnachten = is_christmas();
        let skinning = self.skinning_active();
        let deckel = |pos: &(i32, i32, i32)| self.block_lids.get(pos).map_or(0.0, |d| d.last + (d.progress - d.last) * t);
        for (pos, state) in self.world.model_blocks() {
            let Some(truhe) = world::chest_instance(pos, state, weihnachten) else { continue };
            if !self.block_entity_shown(pos) {
                continue;
            }
            let tex = self.entity_atlas.rect_or_missing(truhe.texture);
            let offen = truhe.partner.map_or(0.0, |p| deckel(&p)).max(deckel(&pos));
            let base = world::entity_model::chest_transform(block_rel(pos, eye), truhe.yaw_deg);
            self.emit_block_entity_model(skinning, pos, world::entity_model::chest_model(truhe.half), &world::entity_model::chest_pose(offen), base, tex, out);
        }
    }

    // ShulkerBoxBlockEntity.updateAnimation / ChestLidAnimator.step: 0.1 pro Tick auf bzw. zu.
    // Fertig geschlossene und abgebaute fliegen raus.
    pub(crate) fn tick_block_lids(&mut self) {
        let world = &self.world;
        self.block_lids.retain(|pos, d| {
            d.last = d.progress;
            d.progress = if d.opening { (d.progress + 0.1).min(1.0) } else { (d.progress - 0.1).max(0.0) };
            (d.opening || d.last > 0.0) && world::block_entity::has_lid(world.block_at(pos.0, pos.1, pos.2))
        });
    }

    // Schilder (SignBlockEntityRenderer): wie die Betten BLOCK-Entities mit leerem
    // Blockmodell, gezeichnet mit Entity-Modell und -Textur. Ohne das fehlen sie
    // ganz. Der Text kommt im UI-Pass (s. build_sign_texts).
    // Augenrelativ wie emit_beds. Frueher standen die Schilder irgendwo im Himmel:
    // hier stand die absolute Blockposition in einem augenrelativen Puffer.
    pub(crate) fn emit_signs(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        for &pos in self.signs.keys() {
            if !self.block_entity_shown(pos) {
                continue;
            }
            let Some((holz, art, grad)) = self.sign_at(pos) else { continue };
            let (textur, modell) = match art {
                world::entity_model::SignKind::Hanging(a) => {
                    (world::entity_model::HANGING_SIGN_TEXTURES[holz], world::entity_model::hanging_sign_model(a))
                }
                _ => (world::entity_model::SIGN_TEXTURES[holz], world::entity_model::sign_model(art == world::entity_model::SignKind::Standing)),
            };
            let tex = self.entity_atlas.rect_or_missing(textur);
            let base = world::entity_model::sign_base(block_rel(pos, eye), art, grad);
            let base = world::entity_model::sign_model_transform(base, art);
            world::emit_entity_model(&modell, &world::Pose::new(), base, tex, ENTITY_BRIGHTNESS, out);
        }
    }

    // Holzart, Art und Drehung (Grad, Vanillas getRotationDegrees) des Schildes an `pos`, oder
    // None, wenn dort (inzwischen) keins steht. Stehend und an der Decke haengend: rotation *
    // 22.5; an der Wand: die Blickrichtung (Sued 0, West 90, Nord 180, Ost 270).
    pub(crate) fn sign_at(&self, pos: (i32, i32, i32)) -> Option<(usize, world::entity_model::SignKind, f32)> {
        use world::entity_model::{HangingAttachment, SignKind};
        let state = self.world.block_at(pos.0, pos.1, pos.2);
        let (holz, mut art) = world::entity_model::sign_kind(world::block_name(state)?)?;
        if art == SignKind::Hanging(HangingAttachment::Ceiling) && world::state_props::property(state, "attached") == Some("true") {
            art = SignKind::Hanging(HangingAttachment::CeilingMiddle);
        }
        let grad = match art {
            SignKind::Standing |
SignKind::Hanging(HangingAttachment::Ceiling |
HangingAttachment::CeilingMiddle) => {
                world::state_props::property(state, "rotation").and_then(|r| r.parse::<f32>().ok()).unwrap_or(0.0) * 22.5
            }
            _ => match world::state_props::property(state, "facing") {
                Some("west") => 90.0,
                Some("north") => 180.0,
                Some("east") => 270.0,
                _ => 0.0,
            },
        };
        Some((holz, art, grad))
    }

    // Modell + Grundform + Atlas-Rechteck fuer einen Entity-Typ, oder None wenn es hier
    // (noch) kein Modell dafuer gibt.
    // Ein End-Kristall wie EndCrystalEntityRenderer: das Modell (s. world::end_crystal_model)
    // mit Textur aus dem Resourcepack, BEIDSEITIG -- Vanilla zeichnet ihn mit
    // entityCutoutNoCull, man sieht durch die durchsichtigen Stellen des Glases also auch
    // dessen Rueckseiten. Die Entity-Pipeline hier verwirft Rueckseiten, deshalb wird jedes
    // Dreieck ein zweites Mal andersherum ausgegeben.
    //
    // Das Alter beginnt in Vanilla bei einem Zufallswert (endCrystalAge = nextInt(100000)),
    // damit nicht alle Kristalle im Gleichtakt schweben; hier fest aus der Entity-ID.
    // Ein Pfeil wie ArrowRenderer: das Modell entlang der Flugrichtung gedreht. Die Rotation
    // kommt aus der Zwischenbild-Interpolation, nicht aus dem Tickwert -- ein Pfeil dreht
    // sich waehrend des Fluges staendig (s. world::entity::tick_projectile), und die
    // Tickstufen waeren bei seiner Geschwindigkeit deutlich sichtbar.
    pub(crate) fn emit_arrow(&self, e: &world::Entity, feet: [f32; 3], progress: f32, texture: &'static str, out: &mut Vec<world::BlockVertex>) {
        let tex = self.entity_atlas.rect_or_missing(texture);
        let (yaw, pitch) = (e.render_yaw(progress), e.render_pitch(progress));
        let model = world::entity_model::arrow_model(texture);
        let pose = world::entity_model::Pose::new();
        // Der Abschuss-Versatz geht NUR hier drauf, nicht in feet selbst: die Trefferbox und
        // alles andere, was an der Position haengt, bleibt an der echten Stelle.
        let versatz = e.launch_render_offset(progress);
        let sichtbar = [feet[0] + versatz[0], feet[1] + versatz[1], feet[2] + versatz[2]];
        let base = world::entity_model::arrow_transform(sichtbar, yaw, pitch);
        world::emit_entity_model(&model, &pose, base, tex, ENTITY_BRIGHTNESS, out);
    }

    // Geworfener Dreizack (ThrownTridentRenderer): das Modell aus den Spieldaten, gedreht wie
    // ein Pfeil (s. world::entity_model::thrown_trident_transform). Ohne Modell stand dort der
    // schwarz-pinke Platzhalter.
    pub(crate) fn emit_thrown_trident(&self, e: &world::Entity, feet: [f32; 3], progress: f32, out: &mut Vec<world::BlockVertex>) {
        let Some(model) = world::model_data::layer_model("trident#main") else { return };
        let tex = self.entity_atlas.rect_or_missing(world::entity_model::TRIDENT_TEXTURE);
        let versatz = e.launch_render_offset(progress);
        let sichtbar = [feet[0] + versatz[0], feet[1] + versatz[1], feet[2] + versatz[2]];
        let base = world::entity_model::thrown_trident_transform(sichtbar, e.render_yaw(progress), e.render_pitch(progress));
        world::emit_entity_model(model, &world::entity_model::Pose::new(), base, tex, ENTITY_BRIGHTNESS, out);
    }

    pub(crate) fn emit_end_crystal(&self, id: i32, e: &world::Entity, feet: [f32; 3], progress: f32, out: &mut Vec<world::BlockVertex>) {
        let tex = self.entity_atlas.rect_or_missing(world::entity_model::END_CRYSTAL_TEXTURE);
        // Klein genug, um es je Bild neu zu bauen (vier Teile).
        let model = world::entity_model::end_crystal_model(e.end_crystal_show_bottom);
        let start = (id as u32).wrapping_mul(2_654_435_761) % 100_000;
        let pose = world::entity_model::end_crystal_pose(start as f32 + e.age_ticks(progress));
        let anfang = out.len();
        world::emit_entity_model(&model, &pose, world::entity_model::end_crystal_transform(feet), tex, ENTITY_BRIGHTNESS, out);
        let rueckseiten: Vec<world::BlockVertex> = out[anfang..].chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
        out.extend(rueckseiten);
    }

    // Entity.shouldRender(distance): mittlere Kantenlaenge der Box mal 64 mal
    // renderDistanceMultiplier (GameOptions: Sichtweite / 8, auf 1..2,5 begrenzt). Windkugeln
    // (ExplosiveProjectileEntity) rechnen die Kante noch mal 4. `feet` ist schon
    // kamerarelativ.
    pub(crate) fn projectile_in_render_range(&self, e: &world::Entity, feet: [f32; 3], faktor: f64) -> bool {
        let (breite, hoehe) = e.hit_size();
        let kante = (breite * 2.0 + hoehe) / 3.0;
        let multiplikator = (f64::from(self.settings.max_render_distance) / 8.0).clamp(1.0, 2.5);
        let grenze = kante * faktor * 64.0 * multiplikator;
        let d2 = feet.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>();
        d2 < grenze * grenze
    }

    // Eine Windkugel wie WindChargeEntityRenderer (s. world::entity_model::wind_charge_model).
    // Ohne Modell stand dort der
    // Platzhalter-Kasten. Beidseitig wie der Kristall: die durchsichtigen Stellen des
    // Wirbels zeigen in Vanilla die Innenseiten.
    pub(crate) fn emit_wind_charge(&self, e: &world::Entity, feet: [f32; 3], progress: f32, out: &mut Vec<world::BlockVertex>) {
        let tex = self.entity_atlas.rect_or_missing(world::entity_model::WIND_CHARGE_TEXTURE);
        let model = world::entity_model::wind_charge_model();
        let pose = world::entity_model::wind_charge_pose(e.age_ticks(progress));
        let anfang = out.len();
        world::emit_entity_model(&model, &pose, world::entity_model::Affine::translation(feet), tex, ENTITY_BRIGHTNESS, out);
        let rueckseiten: Vec<world::BlockVertex> = out[anfang..].chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
        out.extend(rueckseiten);
    }

    pub(crate) fn model_for_type(&self, entity_type: i32) -> Option<(&world::Model, world::entity_model::ModelKind, [f32; 4])> {
        let name = world::entity_type_name(entity_type)?;
        let (_, model, kind) = self.entity_models.iter().find(|(n, _, _)| *n == name)?;
        let tex = self.entity_atlas.rect_or_missing(model.texture);
        Some((model, *kind, tex))
    }
}
