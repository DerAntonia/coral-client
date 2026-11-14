// Icons, Logos, Hintergrundbilder, Weltansicht lesen, GUI-Glint und Sprite-Atlanten.

use super::*;

impl GpuState {
    // Das gerade in offscreen_view stehende Weltbild als Menue-Hintergrund bereitstellen (s.
    // world_backdrop). Die Bind-Group haengt an der View und bleibt bis zum naechsten Resize.
    pub(crate) fn keep_world_backdrop(&mut self) {
        if self.world_backdrop.is_some() {
            return;
        }
        self.world_backdrop = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world backdrop bind group"),
            layout: &self.sprite_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&self.offscreen_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.wallpaper_sampler) },
            ],
        }));
    }

    // Neues Hintergrundbild hochladen (None = keins mehr).
    // Ein Server-Icon (64x64 RGBA) in Platz `slot` des Icon-Atlas legen.
    // Ecken abgerundet wie die Flaeche darunter (Radius 6 auf dem Bildschirm, s.
    // upload_pack_icon).
    pub(crate) fn upload_server_icon(&mut self, slot: usize, rgba: &[u8]) {
        let mut rund = rgba.to_vec();
        bleed_icon_edges(&mut rund, server_status::ICON_SIZE);
        round_icon_corners(&mut rund, server_status::ICON_SIZE, 6.0 * server_status::ICON_SIZE as f32 / server_icon_px());
        let mut atlas = self.server_icons.take();
        self.upload_icon(&mut atlas, "server icons", SERVER_ICON_GRID, server_status::ICON_SIZE, slot, &rund);
        self.server_icons = atlas;
    }

    // Ein Pack-Bild (skin_tab::ICON_PX zum Quadrat, RGBA) in Platz `slot` des Pack-Atlas. Die
    // Ecken werden dabei abgerundet wie die Flaechen darunter und darueber (Radius 6 auf dem
    // Bildschirm) -- ein Sprite selbst kennt keine runden Ecken.
    pub(crate) fn upload_pack_icon(&mut self, slot: usize, rgba: &[u8]) {
        let mut rund = rgba.to_vec();
        bleed_icon_edges(&mut rund, skin_tab::ICON_PX);
        let anzeige = skin_icon_rect(skin_tile_rect(skin_tab::Column::Available, 0.0)).w.max(1.0);
        round_icon_corners(&mut rund, skin_tab::ICON_PX, 6.0 * skin_tab::ICON_PX as f32 / anzeige);
        let mut atlas = self.pack_icons.take();
        self.upload_icon(&mut atlas, "pack icons", skin_tab::ICON_GRID, skin_tab::ICON_PX, slot, &rund);
        self.pack_icons = atlas;
    }

    // Die Logos aus SIDEBAR_LINKS in ihren Atlas, Platz = Index.
    pub(crate) fn upload_logos(&mut self) {
        for (slot, (_, _, png)) in SIDEBAR_LINKS.iter().enumerate() {
            let Ok(bild) = image::load_from_memory_with_format(png, image::ImageFormat::Png) else { continue };
            let mut atlas = self.logos.take();
            self.upload_icon(&mut atlas, "logos", LOGO_GRID, LOGO_PX, slot, &bild.to_rgba8());
            self.logos = atlas;
        }
    }

    // Gemeinsam fuer alle Icon-Atlanten: `n` x `n` Plaetze zu `s` Pixeln, angelegt beim
    // ersten Bild.
    pub(crate) fn upload_icon(&self, atlas: &mut Option<(wgpu::Texture, wgpu::BindGroup)>, label: &str, n: u32, s: u32, slot: usize, rgba: &[u8]) {
        if atlas.is_none() {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: n * s, height: n * s, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.sprite_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.wallpaper_sampler) },
                ],
            });
            *atlas = Some((texture, bind_group));
        }
        let Some((texture, _)) = atlas.as_ref() else { return };
        let (x, y) = ((slot as u32 % n) * s, (slot as u32 / n) * s);
        if rgba.len() != (s * s * 4) as usize || slot as u32 >= n * n {
            return;
        }
        self.queue.write_texture(
            wgpu::ImageCopyTexture { texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(s * 4), rows_per_image: Some(s) },
            wgpu::Extent3d { width: s, height: s, depth_or_array_layers: 1 },
        );
    }

    pub(crate) fn set_panorama(&mut self, bild: Option<&wallpaper::Decoded>) {
        self.panorama = bild.map(|b| self.upload_background("panorama", b, wgpu::TextureFormat::Rgba8UnormSrgb));
    }

    pub(crate) fn set_wallpaper(&mut self, bild: Option<&wallpaper::Decoded>) {
        self.wallpaper = bild.map(|b| self.upload_background("wallpaper", b, wgpu::TextureFormat::Rgba8UnormSrgb));
    }

    // Ein ganzes Bild als eigene Sprite-Textur (Hintergrundbild, letzter Weltblick).
    pub(crate) fn upload_background(&self, label: &str, b: &wallpaper::Decoded, format: wgpu::TextureFormat) -> (wgpu::BindGroup, (u32, u32)) {
        let extent = wgpu::Extent3d { width: b.width, height: b.height, depth_or_array_layers: 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::ImageCopyTexture { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &b.rgba,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(b.width * 4), rows_per_image: Some(b.height) },
            extent,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.sprite_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.wallpaper_sampler) },
            ],
        });
        (bind_group, (b.width, b.height))
    }

    // Letzten Weltblick hochladen (s. last_view.rs). Die Bytes stammen roh aus offscreen_texture,
    // also dieselbe sRGB-Deutung wie dort -- sonst saehe das Bild heller oder dunkler aus als die
    // Welt hinter dem Pausemenue.
    pub(crate) fn set_last_view(&mut self, b: &wallpaper::Decoded) {
        let format = if self.config.format.is_srgb() { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
        self.last_view = Some(self.upload_background("last view", b, format));
    }

    // Dasselbe fuer den Verbindungsbildschirm (s. App::connect_view_shown).
    pub(crate) fn set_connect_view(&mut self, b: &wallpaper::Decoded) {
        let format = if self.config.format.is_srgb() { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
        self.connect_view = Some(self.upload_background("connect view", b, format));
    }

    // Das gerade in offscreen_texture stehende Weltbild in den Speicher lesen, falls es eins ohne
    // HUD ist (s. offscreen_world_only). Wartet auf die GPU -- laeuft nur beim Verlassen einer
    // Welt, nie im Frame.
    pub(crate) fn read_world_view(&self) -> Option<wallpaper::Decoded> {
        if !self.offscreen_world_only {
            return None;
        }
        let bgra = match self.config.format {
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
            _ => return None,
        };
        let (w, h) = (self.offscreen_texture.width(), self.offscreen_texture.height());
        let zeile = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let puffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("last view readback"),
            size: u64::from(zeile) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("last view readback") });
        encoder.copy_texture_to_buffer(
            self.offscreen_texture.as_image_copy(),
            wgpu::ImageCopyBuffer { buffer: &puffer, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(zeile), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        puffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::Maintain::Wait);
        rx.recv().ok()?.ok()?;
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        {
            let daten = puffer.slice(..).get_mapped_range();
            for y in 0..h as usize {
                let z = &daten[y * zeile as usize..][..w as usize * 4];
                for p in z.chunks_exact(4) {
                    // Alpha fest auf deckend: was im Alphakanal des Weltbilds steht, ist kein
                    // Bildinhalt.
                    let px = if bgra { [p[2], p[1], p[0], 255] } else { [p[0], p[1], p[2], 255] };
                    rgba.extend_from_slice(&px);
                }
            }
        }
        puffer.unmap();
        Some(wallpaper::Decoded { rgba, width: w, height: h })
    }

    // Glint-Textur des GUI-Glints (neu nach einem Pack-Wechsel).
    pub(crate) fn set_gui_glint_texture(&mut self, textur: &Option<assets::DecodedTexture>) {
        let (rgba, w, h) = glint_texture_part(textur);
        let view = render::glint::upload_texture(&self.device, &self.queue, rgba, w, h);
        self.glint_gui_bind_group = create_gui_glint_bind_group(
            &self.device,
            &self.glint_gui_bind_group_layout,
            &view,
            &self.glint_gui_sampler,
            &self.glint_gui_uniform,
        );
    }

    // Einmal pro Frame, wie render::BlockRenderer::update_glint.
    pub(crate) fn update_gui_glint(&self, ms: u64, speed: f32, strength: f32) {
        let uniform = render::glint::GlintUniform { rows: render::glint::glint_matrix(ms, speed), params: [strength, 0.0, 0.0, 0.0] };
        self.queue.write_buffer(&self.glint_gui_uniform, 0, bytemuck::bytes_of(&uniform));
    }

    // Nach einem Resourcepack-Wechsel zeigen beide Bind-Groups auf veraltete Texturen: der
    // Blockatlas wurde neu hochgeladen (rebuild_atlas), der UI-Atlas neu gebaut.
    pub(crate) fn rebuild_sprite_atlases(&mut self, ui_atlas_rgba: &[u8], ui_atlas_size: u32) {
        (self.ui_atlas_bind_group, self.ui_atlas_texture) = create_ui_atlas_bind_group(
            &self.device,
            &self.queue,
            &self.sprite_bind_group_layout,
            &self.sprite_sampler,
            ui_atlas_rgba,
            ui_atlas_size,
        );
        self.block_atlas_sprite_bind_group =
            create_block_sprite_bind_group(&self.device, &self.sprite_bind_group_layout, &self.sprite_sampler, &self.block_renderer);
        self.entity_atlas_sprite_bind_group =
            create_entity_sprite_bind_group(&self.device, &self.sprite_bind_group_layout, &self.sprite_sampler, &self.block_renderer);
    }
}
