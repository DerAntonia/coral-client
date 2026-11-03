// Hand, Glint, Puppe, Geister, Entities, Umrisse, Abbau-Risse und Hitboxen.

use super::*;

impl BlockRenderer {
    // Kamera fuer den Hand-Durchgang. Sichtmatrix = Einheitsmatrix: die Hand wird direkt im
    // SICHTraum aufgebaut (Kamera im Ursprung, Blick entlang -Z), genau wie in Vanilla, wo der
    // Matrix-Stack fuer die Hand mit der invertierten Sichtmatrix beginnt.
    //
    // Das Sichtfeld der Hand ist FEST und hat mit der Einstellung nichts zu tun. Vanilla
    // setzt die Hud-Projektion mit getFov(..., changingFov = false),
    // und getFov gibt bei `changingFov == false` schlicht die Konstante 70 zurueck -- der
    // Einstellwert wird dort ueberhaupt nicht gelesen (GameRenderer, 1.21.11). Die Hand sieht
    // deshalb bei FOV 30 wie bei FOV 110 gleich aus.
    //
    // Frueher stand hier der eingestellte Wert. Bei 70 (Standard) faellt das nicht auf, ab
    // etwa 90 aufwaerts wird der Arm zunehmend gestreckt und wandert aus dem Bild.
    //
    // Projektionsmodus fest 0 (normal): die Warp-Projektionen dieses Clients (Panini &Co)
    // gelten fuer die Welt, nicht fuer die Hand -- Vanilla kennt dort ueberhaupt nur die
    // gewoehnliche Perspektive.
    pub fn update_hand_camera(&self, queue: &wgpu::Queue, aspect: f32) {
        let fov = HAND_FOV_DEGREES.to_radians();
        let proj = hand_projection(aspect);
        let uniform = CameraUniform {
            view: glam::Mat4::IDENTITY.to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            params: [0.0, (fov * 0.5).tan(), aspect, self.daylight],
            // Die Hand zeichnet nie Hitbox-Kanten. Im Shader-Pfad steht hier ihr Licht:
            // x = gepackter Lichtwert (Bits als f32), s. block.wgsl::fs_hand_lit.
            hitbox: [f32::from_bits(self.hand_light.unwrap_or(0)), 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.hand_camera_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn set_hand_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        let n = vertices.len().min(HAND_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.hand_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.hand_vertex_count = n as u32;
    }

    // Eigener Durchgang NACH der Welt, mit geleertem Tiefenpuffer (s. Feldkommentar) und
    // eigener Kamera. Die Hand nutzt dieselbe Pipeline und denselben Entity-Atlas wie die
    // Spielermodelle -- sie IST ein Stueck Spielermodell.
    pub fn render_hand<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.hand_vertex_count == 0 {
            return;
        }
        pass.set_pipeline(if self.hand_light.is_some() { &self.hand_lit_pipeline } else { &self.pipeline });
        pass.set_bind_group(0, &self.hand_camera_bind_group, &[]);
        pass.set_bind_group(1, &self.entity_bind_group, &[]);
        let bytes = u64::from(self.hand_vertex_count) * std::mem::size_of::<BlockVertex>() as u64;
        pass.set_vertex_buffer(0, self.hand_buffer.slice(..bytes));
        pass.set_vertex_buffer(1, self.zero_offset_buffer.slice(..));
        pass.draw(0..self.hand_vertex_count, 0..1);
        self.draw_glint(pass, &self.hand_glint);
    }

    // Glint-Twins ueber dem eben Gezeichneten. Kamera (0) und Entity-Atlas (1) sind vom
    // Aufrufer schon gebunden -- die Maske sampelt denselben Atlas wie das Item.
    pub(super) fn draw_glint<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, twins: &'a crate::glint::GlintBuffer) {
        if twins.is_empty() {
            return;
        }
        pass.set_pipeline(&self.glint_pipeline);
        pass.set_bind_group(2, &self.glint_bind_group, &[]);
        twins.draw(pass, &self.zero_offset_buffer);
    }

    // Die beiden Vanilla-Glint-Texturen (textures/misc/enchanted_glint_item.png und
    // enchanted_glint_armor.png), als RGBA8. Neu gesetzt auch nach einem Resourcepack-Wechsel.
    pub fn set_glint_textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, item: (&[u8], u32, u32), entity: (&[u8], u32, u32)) {
        let item = crate::glint::upload_texture(device, queue, item.0, item.1, item.2);
        let entity = crate::glint::upload_texture(device, queue, entity.0, entity.1, entity.2);
        self.glint_bind_group =
            create_glint_bind_group(device, &self.glint_bind_group_layout, &item, &entity, &self.glint_sampler, &self.glint_uniform_buffer);
    }

    // Einmal pro Frame: die wandernde Texturmatrix (s. glint::glint_matrix). `speed` und
    // `strength` sind Vanillas Optionen glintSpeed/glintStrength (0..1).
    pub fn update_glint(&self, queue: &wgpu::Queue, ms: u64, speed: f32, strength: f32) {
        let uniform = crate::glint::GlintUniform { rows: crate::glint::glint_matrix(ms, speed), params: [strength, 0.0, 0.0, 0.0] };
        queue.write_buffer(&self.glint_uniform_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn set_hand_glint_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        self.hand_glint.set(queue, vertices);
    }

    pub fn set_entity_glint_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        self.entity_glint.set(queue, vertices);
    }

    pub fn set_doll_glint_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        self.doll_glint.set(queue, vertices);
    }

    // Kamera fuer die Spielerfigur im Inventar. `view` und `proj` kommen fertig vom Aufrufer
    // (s. app::doll_camera): Vanilla baut dort keine perspektivische Kamera, sondern eine
    // ORTHOGRAFISCHE -- die Figur im Inventar hat keine Fluchtpunkte, ein Arm vorn ist
    // genauso gross wie einer hinten.
    pub fn update_doll_camera(&self, queue: &wgpu::Queue, view: glam::Mat4, proj: glam::Mat4) {
        let uniform = CameraUniform {
            view: view.to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            // params.x = Projektionsmodus 0 (normal). Die uebrigen Felder braucht nur die
            // Warp-Projektion der Welt; im Orthomodus liest der Shader sie nicht.
            params: [0.0, 1.0, 1.0, 0.0],
            hitbox: [0.0; 4],
        };
        queue.write_buffer(&self.doll_camera_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn set_doll_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        let n = vertices.len().min(DOLL_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.doll_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.doll_vertex_count = n as u32;
    }

    pub const fn has_doll(&self) -> bool {
        self.doll_vertex_count > 0
    }

    // Eigener Durchgang MIT geleertem Tiefenpuffer, wie bei der Hand: die Figur ist echte
    // 3D-Geometrie und braucht den Tiefentest gegen sich selbst (ein Arm vor dem Rumpf),
    // darf aber nichts von der Welt dahinter sehen.
    pub fn render_doll<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.doll_vertex_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.doll_camera_bind_group, &[]);
        pass.set_bind_group(1, &self.entity_bind_group, &[]);
        let bytes = u64::from(self.doll_vertex_count) * std::mem::size_of::<BlockVertex>() as u64;
        pass.set_vertex_buffer(0, self.doll_buffer.slice(..bytes));
        pass.set_vertex_buffer(1, self.zero_offset_buffer.slice(..));
        pass.draw(0..self.doll_vertex_count, 0..1);
        self.draw_glint(pass, &self.doll_glint);
    }

    // Geometrie der unbestaetigten Platzierungen (s. app::ghost_vertices). Wird nur bei
    // Aenderung geschrieben, nicht pro Frame.
    // `solid_count` ist die Anzahl FUEHRENDER Vertices, die deckend gezeichnet werden. Der
    // Aufrufer sortiert also: erst die deckenden, dann die halbdurchsichtigen.
    // Danach `translucent_count` durchscheinende (s. ghost_translucent_count), der Rest ist
    // halbdurchsichtig.
    // `lights`: Shader-Pfad, je Vertex des deckenden Teils ein Lichtwert; leer im PvP-Pfad.
    pub fn set_ghost_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex], solid_count: usize, translucent_count: usize, lights: &[u32]) {
        let n = vertices.len().min(GHOST_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.ghost_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        let l = lights.len().min(n);
        if l > 0 {
            queue.write_buffer(&self.ghost_light_buffer, 0, bytemuck::cast_slice(&lights[..l]));
        }
        self.ghost_light_count = l as u32;
        self.ghost_vertex_count = n as u32;
        self.ghost_solid_count = solid_count.min(n) as u32;
        self.ghost_translucent_count = translucent_count.min(n - self.ghost_solid_count as usize) as u32;
    }

    // `tints` muss GENAU so lang sein wie `vertices` (ein rgb+Anteil je Vertex, s.
    // block.wgsl::vs_entity). Eine kuerzere Liste liesse die GPU ueber das Pufferende hinaus
    // lesen, deshalb wird auf die kleinere der beiden Laengen gekappt statt zu vertrauen.
    //
    // `translucent`: so viele Vertices am Ende laufen durchscheinend, `head` ebenso viele am
    // Anfang (der eigene Spieler beim Ausblenden, s. block.wgsl::entity_fade).
    // `lights`: Shader-Pfad, je Vertex ein Lichtwert (s. entity_light_buffer); leer im PvP-Pfad.
    // `own`: so viele Vertices am Anfang sind der eigene Spieler (nur in der 3. Person, s.
    // render_own_mask).
    pub fn set_entity_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex], tints: &[[f32; 4]], lights: &[u32], (head, translucent, own): (usize, usize, usize)) {
        debug_assert_eq!(vertices.len(), tints.len(), "je Entity-Vertex genau ein Tint-Eintrag");
        let n = vertices.len().min(tints.len()).min(ENTITY_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.entity_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
            queue.write_buffer(&self.entity_tint_buffer, 0, bytemuck::cast_slice(&tints[..n]));
        }
        self.entity_light_count = 0;
        if n > 0 && lights.len() >= n {
            queue.write_buffer(&self.entity_light_buffer, 0, bytemuck::cast_slice(&lights[..n]));
            self.entity_light_count = n as u32;
        }
        self.entity_vertex_count = n as u32;
        self.entity_translucent_count = translucent.min(n) as u32;
        self.entity_head_translucent = head.min(n - translucent.min(n)) as u32;
        self.own_vertex_count = own.min(n) as u32;
    }

    pub const fn has_own_player(&self) -> bool {
        self.own_vertex_count > 0
    }

    // Der eigene Spieler in die Maske der Bewegungsunschaerfe (Pass mit der Tiefe der Welt,
    // nur gelesen; Ziel motion_blur::MASK_FORMAT).
    pub fn render_own_mask<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.own_vertex_count == 0 {
            return;
        }
        let n = u64::from(self.own_vertex_count);
        pass.set_pipeline(&self.own_mask_pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.entity_bind_group, &[]);
        pass.set_vertex_buffer(0, self.entity_buffer.slice(..n * std::mem::size_of::<BlockVertex>() as u64));
        pass.set_vertex_buffer(1, self.entity_tint_buffer.slice(..n * std::mem::size_of::<[f32; 4]>() as u64));
        pass.draw(0..self.own_vertex_count, 0..1);
    }

    // Bereiche der Entity-Vertices, die man lesen koennen soll (Karten in Rahmen, Schilder):
    // nach der Bewegungsunschaerfe scharf darueber (s. render_sharp_entities).
    pub fn set_sharp_entities(&mut self, ranges: Vec<std::ops::Range<u32>>) {
        self.sharp_entity_ranges = ranges;
    }

    // Die lesbaren Bereiche noch einmal mit der Entity-Pipeline, gegen die stehengebliebene
    // Tiefe der Welt (LessEqual: genau dieselben Pixel wie im Welt-Durchgang). Nur deckende
    // Vertices -- der eigene Spieler beim Ausblenden und das Durchscheinende am Ende nicht.
    pub fn render_sharp_entities<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let deckend = self.entity_head_translucent..self.entity_vertex_count - self.entity_translucent_count;
        if self.sharp_entity_ranges.is_empty() || deckend.is_empty() {
            return;
        }
        let n = u64::from(self.entity_vertex_count);
        pass.set_pipeline(&self.entity_pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.entity_bind_group, &[]);
        pass.set_vertex_buffer(0, self.entity_buffer.slice(..n * std::mem::size_of::<BlockVertex>() as u64));
        pass.set_vertex_buffer(1, self.entity_tint_buffer.slice(..n * std::mem::size_of::<[f32; 4]>() as u64));
        for r in &self.sharp_entity_ranges {
            let (a, b) = (r.start.max(deckend.start), r.end.min(deckend.end));
            if a < b {
                pass.draw(a..b, 0..1);
            }
        }
    }

    // Silhouetten leuchtender Entities, je Vertex die Umrissfarbe (rgb, 0..1 wie in Vanilla).
    pub fn set_outline_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex], colors: &[[f32; 4]]) {
        let n = vertices.len().min(colors.len()).min(OUTLINE_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.outline_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
            queue.write_buffer(&self.outline_color_buffer, 0, bytemuck::cast_slice(&colors[..n]));
        }
        self.outline_vertex_count = n as u32;
    }

    pub const fn has_outlines(&self) -> bool {
        self.outline_vertex_count > 0
    }

    // Die Silhouetten in das Umriss-Bild (Pass ohne Tiefe, Ziel entity_outline::FORMAT).
    pub fn render_outline_silhouettes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.outline_vertex_count == 0 {
            return;
        }
        let n = u64::from(self.outline_vertex_count);
        pass.set_pipeline(&self.outline_pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.entity_bind_group, &[]);
        pass.set_vertex_buffer(0, self.outline_buffer.slice(..n * std::mem::size_of::<BlockVertex>() as u64));
        pass.set_vertex_buffer(1, self.outline_color_buffer.slice(..n * std::mem::size_of::<[f32; 4]>() as u64));
        pass.draw(0..self.outline_vertex_count, 0..1);
    }

    pub fn set_crumbling_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        let n = vertices.len().min(CRUMBLING_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.crumbling_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.crumbling_vertex_count = n as u32;
    }

    pub fn set_hitbox_vertices(&mut self, queue: &wgpu::Queue, vertices: &[HitboxVertex]) {
        let n = vertices.len().min(HITBOX_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.hitbox_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.hitbox_vertex_count = n as u32;
    }

    // Kanten des Blockumrisses im Modus Komplementaer (s. OutlineComplement); leer = keiner.
    pub fn set_outline_complement_vertices(&mut self, queue: &wgpu::Queue, vertices: &[HitboxVertex]) {
        self.outline_complement.set_vertices(queue, vertices);
    }

    // Nach dem Welt-Durchgang: das Bild kopieren und den Umriss in der Gegenfarbe darueber,
    // gegen die Tiefe der Welt. `color` ist das Ziel des Welt-Durchgangs, `color_view` eine
    // Ansicht davon im Format der Pipeline.
    pub fn encode_outline_complement(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, color: &wgpu::Texture, color_view: &wgpu::TextureView) {
        self.outline_complement.encode(device, encoder, color, color_view, &self.depth_view, &self.camera_bind_group);
    }

    // Bildschirmgroesse fuer die Hitbox-Anzeige. Landet im Kamera-Uniform (s. update_camera)
    // -- der Vertex-Shader verbreitert die Kanten erst nach der Projektion und braucht sie
    // dort. Breite und Deckkraft tragen die Vertices selbst (s. HitboxVertex).
    pub const fn set_hitbox_params(&mut self, screen: (f32, f32)) {
        self.hitbox_params = [0.0, HITBOX_MIN_RASTER_HALF_PX, screen.0, screen.1];
    }
}
