// BlockRenderer::new: Pipelines, Buffer, Texturen und Bind-Groups.

use super::*;

impl BlockRenderer {
    // `atlas_rgba` ist ein quadratischer RGBA8-Block der Kantenlaenge `atlas_size` (der
    // Block-/Entity-Texturatlas). Wird einmal zur GPU hochgeladen und nie wieder geaendert.
    #[must_use] 
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        atlas_mips: &[Vec<u8>],
        atlas_size: u32,
        // Eigener Atlas fuer Entity-Texturen (Skins, Mob-Texturen) -- gleiche Pipeline,
        // andere Textur (s. render()). Ohne Mipmaps: Entities sind meist nah, und Vanilla
        // sampelt Entity-Texturen ebenfalls ohne Mip-Kette.
        entity_atlas_rgba: &[u8],
        entity_atlas_size: u32,
        // Partikel-Sprites (assets::ParticleAtlas), ebenfalls ohne Mip-Kette.
        particle_atlas_rgba: &[u8],
        particle_atlas_size: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("block shader"),
            source: wgpu::ShaderSource::Wgsl(crate::block_shader_source().into()),
        });

        let atlas_bind_group_layout = create_atlas_bind_group_layout(device);
        let (atlas_bind_group, atlas_bind_group_flat, atlas_texture) =
            create_atlas_texture_and_bind_group(device, queue, atlas_mips, atlas_size, &atlas_bind_group_layout);
        // Derselbe Layout -- deshalb laesst sich die Entity-Textur ohne zweite Pipeline
        // einfach an derselben Stelle binden.
        let (entity_bind_group, _, entity_atlas_texture) = create_atlas_texture_and_bind_group(
            device,
            queue,
            std::slice::from_ref(&entity_atlas_rgba.to_vec()),
            entity_atlas_size,
            &atlas_bind_group_layout,
        );

        let (particle_bind_group, _, _particle_atlas_texture) = create_atlas_texture_and_bind_group(
            device,
            queue,
            std::slice::from_ref(&particle_atlas_rgba.to_vec()),
            particle_atlas_size,
            &atlas_bind_group_layout,
        );

        let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // Auch FRAGMENT, nicht nur VERTEX: fs_main_hitbox liest camera.params.w als
                // Hitbox-Deckkraft (s. dortigen Kommentar). Alle anderen Fragment-Shader
                // dieser Datei ignorieren das Uniform ohnehin, die zusaetzliche Sichtbarkeit
                // kostet sie nichts.
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera uniform buffer"),
            size: std::mem::size_of::<CameraUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: camera_buffer.as_entire_binding() }],
        });

        // Zweite Kamera fuer die Ego-Hand: gleiche Struktur, gleicher Layout, eigener Inhalt
        // (s. update_hand_camera). Ein zweiter Puffer statt eines Umschreibens pro Frame,
        // weil Welt und Hand im SELBEN Encoder gezeichnet werden -- ein Umschreiben zwischen
        // den beiden Durchgaengen wuerde auch rueckwirkend die schon aufgezeichneten
        // Zeichenbefehle der Welt treffen.
        let hand_camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hand camera uniform buffer"),
            size: std::mem::size_of::<CameraUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hand_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hand camera bind group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: hand_camera_buffer.as_entire_binding() }],
        });

        let doll_camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("doll camera uniform buffer"),
            size: std::mem::size_of::<CameraUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let doll_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("doll camera bind group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: doll_camera_buffer.as_entire_binding() }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("block pipeline layout"),
            bind_group_layouts: &[&camera_bind_group_layout, &atlas_bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BlockVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 6]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };

        // Zweiter Vertex-Puffer, INSTANZ-Rate: ein einziger vec3 je Zeichenaufruf, der sagt,
        // wo die Spalte relativ zum Auge steht (s. block.wgsl). Instanz-Rate statt eines
        // Uniforms, weil ein Uniform mit dynamischem Versatz auf 256 Byte ausgerichtet sein
        // muesste -- fuer 12 Byte Nutzlast je Spalte waere das das Zwanzigfache an Speicher
        // und ein zusaetzlicher Bindgroup-Wechsel pro Spalte.
        let offset_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: COLUMN_OFFSET_STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 5, format: wgpu::VertexFormat::Float32x3 },
            ],
        };

        // Dritter Vertex-Puffer, VERTEX-Rate: die Schadens-Faerbung je Entity-Vertex
        // (rgb + Anteil, s. block.wgsl::vs_entity). Er belegt shader_location 3, dieselbe
        // Stelle, an der die Chunk-Pipeline ihren Spalten-Versatz hat -- Entities brauchen
        // den nicht, sie kommen schon augenrelativ an.
        //
        // Pro Vertex statt pro Entity, weil alle Entities in EINEM Zeichenaufruf stecken:
        // ein Uniform waere fuer alle gleich, und ein Aufruf je Entity waere teurer als die
        // 16 Byte, die hier je Vertex dazukommen.
        let entity_tint_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x4 }],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("block pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Rueckseitige Flaechen verwerfen. Grob die Haelfte aller ausgegebenen Quads
            // zeigt vom Betrachter weg (die abgewandten Seiten jeder Chunk-Spalte) und wurde
            // bisher voll rasterisiert. Vanilla macht dasselbe: RenderPipeline.Builder hat
            // `cull.orElse(true)`, und die Terrain-Pipelines (SOLID_TERRAIN/CUTOUT_TERRAIN)
            // schalten es -- anders als Entities/Wolken -- nie ab.
            //
            // Voraussetzung ist eine EINHEITLICHE Wicklung: push_face_quad gibt alle sechs
            // Wuerfelseiten gegen den Uhrzeigersinn von aussen gesehen aus (nachgerechnet,
            // s. Test wuerfelflaechen_winden_konsistent_nach_aussen im world-Crate), und
            // push_cross_quads gibt Pflanzen seit dieser Aenderung beidseitig aus.
            //
            // front_face bleibt beim Default Ccw. Stimmt die Haendigkeit der Projektion
            // nicht wie erwartet, waere die Welt sichtbar auf links gedreht (man saehe die
            // Innenseiten) -- dann ist Some(Front) die Korrektur, nicht None.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let terrain_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Uint16x4 },
                wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Unorm8x4 },
                wgpu::VertexAttribute { offset: 12, shader_location: 2, format: wgpu::VertexFormat::Uint16x2 },
            ],
        };
        let terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_terrain",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[terrain_vertex_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Wie `pipeline` (s. dort zu cull_mode und Wicklung): pack_terrain behaelt die
            // Reihenfolge der Ecken, das Index-Muster 0,1,2, 0,2,3 die Wicklung.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Shader-Pfad: gleiche Geometrie und Tiefe, aber zwei Farbziele (Albedo, Normale).
        let gbuffer_terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain gbuffer pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_terrain_gbuffer",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[terrain_vertex_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_terrain_gbuffer",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: GBUFFER_ALBEDO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: GBUFFER_NORMAL_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: GBUFFER_LIGHT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let quad_index_capacity = 1 << 16;
        let quad_index_buffer = create_quad_index_buffer(device, queue, quad_index_capacity);

        // Translucent-Pipeline (Wasser UND Glas): echtes Alpha-Blending (SrcAlpha/
        // OneMinusSrcAlpha) statt REPLACE -- sonst identisch zur Opak-Pipeline (gleiches
        // Vertex-Layout, gleiche Bind-Groups).
        let translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("block translucent pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main_translucent",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Rueckseiten verwerfen wie in der Opak-Pipeline -- und wie in Vanilla, dessen
            // TRANSLUCENT-Terrain-Pipeline ebenfalls kein withCull(false) setzt.
            //
            // Das ist fuer GLAS zwingend: ohne Culling zeichnet jeder Glasblock Vorder- UND
            // Rueckseite, beide blenden, und das Glas wirkt doppelt so dicht wie in Vanilla.
            // Wasser bleibt trotzdem von beiden Seiten sichtbar (man schwimmt darunter) --
            // seine Doppelseitigkeit steckt jetzt in der GEOMETRIE statt in diesem
            // Pipeline-Zustand, s. world::mesher::push_rueckseiten. Genau so macht es
            // Vanillas FluidRenderer auch.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // KEIN Tiefenschreiben -- BEWUSSTE Abweichung von Vanilla, dessen
                // TRANSLUCENT-Pipeline die Voreinstellung `writeDepth.orElse(true)` behaelt.
                //
                // Einmal auf true gestellt und sofort wieder zurueckgenommen: Vanilla kann
                // sich Tiefenschreiben nur leisten, weil es die durchsichtigen Quads JE
                // SEKTION nach Abstand sortiert. Ohne diese Sortierung blockiert die zuerst
                // gezeichnete durchsichtige Flaeche alles dahinter -- Wasser verschwand
                // hinter Glas, obwohl man hindurchsehen koennen muss. Ordnung her, DANN
                // Tiefenschreiben; nie umgekehrt.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Wasser/Glas der Spalten im kompakten Format (s. world::terrain_vertex) -- sonst wie
        // translucent_pipeline, das die Geisterbloecke weiter im alten Format benutzen.
        let terrain_translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain translucent pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_terrain",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[terrain_vertex_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main_translucent",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Rueckseiten verwerfen wie in der Opak-Pipeline -- und wie in Vanilla, dessen
            // TRANSLUCENT-Terrain-Pipeline ebenfalls kein withCull(false) setzt.
            //
            // Das ist fuer GLAS zwingend: ohne Culling zeichnet jeder Glasblock Vorder- UND
            // Rueckseite, beide blenden, und das Glas wirkt doppelt so dicht wie in Vanilla.
            // Wasser bleibt trotzdem von beiden Seiten sichtbar (man schwimmt darunter) --
            // seine Doppelseitigkeit steckt jetzt in der GEOMETRIE statt in diesem
            // Pipeline-Zustand, s. world::mesher::push_rueckseiten. Genau so macht es
            // Vanillas FluidRenderer auch.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // KEIN Tiefenschreiben -- BEWUSSTE Abweichung von Vanilla, dessen
                // TRANSLUCENT-Pipeline die Voreinstellung `writeDepth.orElse(true)` behaelt.
                //
                // Einmal auf true gestellt und sofort wieder zurueckgenommen: Vanilla kann
                // sich Tiefenschreiben nur leisten, weil es die durchsichtigen Quads JE
                // SEKTION nach Abstand sortiert. Ohne diese Sortierung blockiert die zuerst
                // gezeichnete durchsichtige Flaeche alles dahinter -- Wasser verschwand
                // hinter Glas, obwohl man hindurchsehen koennen muss. Ordnung her, DANN
                // Tiefenschreiben; nie umgekehrt.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Shader-Pfad: dasselbe mit Licht je Vertex (s. block.wgsl::vs_terrain_lit).
        let lit_terrain_translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lit terrain translucent pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_terrain_lit",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[terrain_vertex_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_terrain_translucent_lit",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Rueckseiten verwerfen wie in der Opak-Pipeline -- und wie in Vanilla, dessen
            // TRANSLUCENT-Terrain-Pipeline ebenfalls kein withCull(false) setzt.
            //
            // Das ist fuer GLAS zwingend: ohne Culling zeichnet jeder Glasblock Vorder- UND
            // Rueckseite, beide blenden, und das Glas wirkt doppelt so dicht wie in Vanilla.
            // Wasser bleibt trotzdem von beiden Seiten sichtbar (man schwimmt darunter) --
            // seine Doppelseitigkeit steckt jetzt in der GEOMETRIE statt in diesem
            // Pipeline-Zustand, s. world::mesher::push_rueckseiten. Genau so macht es
            // Vanillas FluidRenderer auch.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // KEIN Tiefenschreiben -- BEWUSSTE Abweichung von Vanilla, dessen
                // TRANSLUCENT-Pipeline die Voreinstellung `writeDepth.orElse(true)` behaelt.
                //
                // Einmal auf true gestellt und sofort wieder zurueckgenommen: Vanilla kann
                // sich Tiefenschreiben nur leisten, weil es die durchsichtigen Quads JE
                // SEKTION nach Abstand sortiert. Ohne diese Sortierung blockiert die zuerst
                // gezeichnete durchsichtige Flaeche alles dahinter -- Wasser verschwand
                // hinter Glas, obwohl man hindurchsehen koennen muss. Ordnung her, DANN
                // Tiefenschreiben; nie umgekehrt.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Geisterbloecke: eigene Pipeline, weil das halbe Alpha im Fragment-Shader festgesetzt
        // wird (fs_main_ghost) statt aus der Textur zu kommen -- ein Obsidianblock hat in der
        // Textur Alpha 1.0 und waere in der Wasser-Pipeline genauso undurchsichtig wie sonst.
        // Tiefe wird getestet, aber nicht geschrieben: der Geisterblock soll hinter echter
        // Geometrie verschwinden, aber nichts dahinter wegblockieren.
        let ghost_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("block ghost pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main_ghost",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Glint (s. glint.rs): dieselbe Geometrie wie das Item (vs_main), eigener Fragment-
        // Shader, additiv gemischt, ohne Tiefenschreiben. Tiefe LESS_EQUAL mit einem kleinen
        // Bias zur Kamera hin statt Vanillas EQUAL -- warum, steht in glint.rs. Rueckseiten
        // verworfen wie beim Item selbst, damit der Glint nur auf Sichtbarem liegt.
        let glint_bind_group_layout = crate::glint::create_bind_group_layout(device);
        let glint_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glint pipeline layout"),
            bind_group_layouts: &[&camera_bind_group_layout, &atlas_bind_group_layout, &glint_bind_group_layout],
            push_constant_ranges: &[],
        });
        let glint_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glint pipeline"),
            layout: Some(&glint_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_glint",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        // BlendFunction.GLINT: Alpha (ZERO, ONE) -- das Ziel behaelt sein Alpha.
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState { constant: -2, slope_scale: -1.0, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let glint_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glint uniform buffer"),
            size: std::mem::size_of::<crate::glint::GlintUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glint_sampler = crate::glint::create_sampler(device);
        // Bis die App die echten Texturen setzt (set_glint_textures): weiss.
        let glint_bind_group = create_glint_bind_group(
            device,
            &glint_bind_group_layout,
            &crate::glint::upload_texture(device, queue, &[255; 4], 1, 1),
            &crate::glint::upload_texture(device, queue, &[255; 4], 1, 1),
            &glint_sampler,
            &glint_uniform_buffer,
        );

        // Hitbox-Anzeige (Modul, s. app::build_hitbox_vertices): eigenes Vertex-Format
        // (HitboxVertex) und eigene Entry-Points -- die Kanten kommen als ENDPUNKT-PAARE an
        // und werden erst nach der Projektion auf Pixelbreite gebracht (s. vs_hitbox).
        //
        // Tiefe testen, aber nicht schreiben (wie ghost_pipeline): die Anzeige verschwindet
        // hinter echten Bloecken, verdeckt aber nichts hinter sich.
        //
        // KEIN Backface-Culling: nach der Verbreiterung im Bildschirmraum haengt die
        // Wicklungsrichtung des Bandes davon ab, aus welcher Richtung man die Kante sieht --
        // mit Culling waere jede Kante aus der halben Blickrichtung unsichtbar.
        let hitbox_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<HitboxVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 6]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        };
        // Entity-Pipeline: BIS AUF den Tint-Puffer (statt des Spalten-Versatzes) und das
        // eigene Shader-Paar identisch zur Block-Pipeline -- gleiche Bind-Groups, gleiches
        // Culling, gleiches Tiefenverhalten, kein Blending. Entities liefen vorher ueber die
        // Block-Pipeline; alles ausser der Faerbung soll sich deshalb nicht aendern.
        let entity_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("entity pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), entity_tint_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Silhouetten leuchtender Entities: dieselben Vertices und Bindungen wie die Entities,
        // aber in ein eigenes Bild (s. entity_outline.rs) -- ohne Tiefe, damit der Umriss durch
        // Waende sichtbar ist, und beidseitig, weil nur die Flaeche zaehlt.
        let outline_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("entity outline silhouette pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), entity_tint_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_entity_outline",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: crate::entity_outline::FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Maske des eigenen Spielers (Bewegungsunschaerfe, s. crate::motion_blur): dieselbe
        // Vertex-Stufe und Rasterung wie die Entity-Pipeline, die Tiefe nur gelesen -- so
        // treffen genau die Pixel, an denen er im Bild zu sehen ist.
        let own_mask_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("own player mask pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), entity_tint_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_entity_mask",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: crate::motion_blur::MASK_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Wie die Entity-Pipeline, aber gemischt (RenderLayers.entityTranslucent).
        let entity_translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("entity translucent pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), entity_tint_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_entity_translucent",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Shader-Pfad: dieselben beiden Entity-Pipelines mit Licht je Vertex aus einem dritten
        // Puffer (s. entity_light_buffer).
        let entity_light_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<u32>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute { offset: 0, shader_location: 4, format: wgpu::VertexFormat::Uint32 }],
        };
        let lit_entity = |label: &str, fs: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_entity_lit",
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[vertex_buffer_layout.clone(), entity_tint_layout.clone(), entity_light_layout.clone()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: fs,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let lit_entity_pipeline = lit_entity("lit entity pipeline", "fs_entity_lit", wgpu::BlendState::REPLACE);
        let lit_entity_translucent_pipeline = lit_entity("lit entity translucent pipeline", "fs_entity_translucent_lit", wgpu::BlendState::ALPHA_BLENDING);
        // Shader-Pfad: deckende Geisterbloecke in den G-Buffer, damit ein gerade gesetzter Block
        // genauso beleuchtet wird (Sonne, Schatten) wie danach sein Chunk-Mesh.
        let gbuffer_ghost_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ghost gbuffer pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_block_gbuffer",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), entity_light_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_terrain_gbuffer",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: GBUFFER_ALBEDO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: GBUFFER_NORMAL_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: GBUFFER_LIGHT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Shader-Pfad: die Ego-Hand wie die Block-Pipeline, nur mit Licht und Tonemap im
        // Fragment-Shader (das Licht steht im Hand-Kamera-Uniform, s. update_hand_camera).
        let hand_lit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lit hand pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_hand_lit",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Riss-Ueberlagerung beim Abbauen -- 1:1 Vanillas RenderPipelines.CRUMBLING:
        // multiplikativ gemischt (DST_COLOR/SRC_COLOR, also 2*Quelle*Ziel, dunkle Risse
        // dunkeln ab), keine Tiefe geschrieben und ein Tiefen-Bias (-1.0 Steigung,
        // -10.0 konstant), damit die Risse vor der Blockflaeche liegen statt mit ihr zu
        // flackern. Vertex-Seite ist die gewoehnliche Block-Pipeline (die Vertices kommen
        // schon augenrelativ, der Instanz-Versatz ist null).
        let crumbling_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("crumbling pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_crumbling",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Dst,
                            dst_factor: wgpu::BlendFactor::Src,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState { constant: -10, slope_scale: -1.0, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let outline_complement = OutlineComplement::new(device, &shader, &camera_bind_group_layout, hitbox_vertex_layout.clone(), surface_format);
        let hitbox_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hitbox pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_hitbox",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[hitbox_vertex_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_hitbox",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Partikel: kein Backface-Culling (ein Billboard hat keine Rueckseite, aber die
        // Wicklung haengt an der Kameradrehung), Tiefe wird getestet. Die opake Variante
        // schreibt Tiefe wie Vanillas OPAQUE_PARTICLE, die durchscheinende nicht -- ungeordnet
        // gemischte Quads wuerden sich sonst gegenseitig wegschneiden (dieselbe Abwaegung wie
        // beim Wasser, s. translucent_pipeline).
        let particle_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 7]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };
        let weather_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<WeatherVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 28, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 36, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
            ],
        };
        let quad_pipeline = |label: &str, vertex: (&str, &wgpu::VertexBufferLayout), entry: &str, blend: wgpu::BlendState, write_depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: vertex.0,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: std::slice::from_ref(vertex.1),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: entry,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: write_depth,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let particle_pipeline = |label: &str, entry: &str, blend: wgpu::BlendState, write_depth: bool| {
            quad_pipeline(label, ("vs_particle", &particle_vertex_layout), entry, blend, write_depth)
        };
        let particle_opaque_pipeline = particle_pipeline("particle opaque pipeline", "fs_particle_opaque", wgpu::BlendState::REPLACE, true);
        let particle_translucent_pipeline =
            particle_pipeline("particle translucent pipeline", "fs_particle_translucent", wgpu::BlendState::ALPHA_BLENDING, false);
        let weather_pipeline =
            quad_pipeline("weather pipeline", ("vs_weather", &weather_vertex_layout), "fs_weather", wgpu::BlendState::ALPHA_BLENDING, false);
        let weather_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("weather vertex buffer"),
            size: (std::mem::size_of::<WeatherVertex>() * WEATHER_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let particle_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle vertex buffer"),
            size: (std::mem::size_of::<ParticleVertex>() * PARTICLE_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let depth_view = create_depth_view(device, width, height);

        // Ein paar wenige Bloecke gleichzeitig -- mehr als eine Handvoll unbestaetigter
        // Platzierungen gibt es nicht, dafuer reicht ein kleiner, fest vergebener Puffer.
        let ghost_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ghost vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * GHOST_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let entity_tint_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity tint buffer"),
            size: (std::mem::size_of::<[f32; 4]>() * ENTITY_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ghost_light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ghost light buffer"),
            size: (std::mem::size_of::<u32>() * GHOST_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let entity_light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity light buffer"),
            size: (std::mem::size_of::<u32>() * ENTITY_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let outline_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity outline vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * OUTLINE_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let outline_color_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity outline color buffer"),
            size: (std::mem::size_of::<[f32; 4]>() * OUTLINE_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let entity_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * ENTITY_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let crumbling_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("crumbling vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * CRUMBLING_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let hitbox_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hitbox vertex buffer"),
            size: (std::mem::size_of::<HitboxVertex>() * HITBOX_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let hand_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hand vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * HAND_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let doll_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("doll vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * DOLL_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            terrain_pipeline,
            quad_index_buffer,
            quad_index_capacity,
            translucent_pipeline,
            ghost_pipeline,
            ghost_buffer,
            ghost_vertex_count: 0,
            ghost_solid_count: 0,
            ghost_translucent_count: 0,
            crumbling_pipeline,
            crumbling_buffer,
            crumbling_vertex_count: 0,
            hitbox_pipeline,
            hitbox_buffer,
            hitbox_vertex_count: 0,
            hitbox_params: [0.0, HITBOX_MIN_RASTER_HALF_PX, width as f32, height as f32],
            outline_complement,
            depth_view,
            camera_buffer,
            camera_bind_group,
            atlas_bind_group,
            atlas_bind_group_flat,
            mipmapping: true,
            entity_bind_group,
            vanilla: crate::vanilla_draw::VanillaDraw::new(device, queue, surface_format, &entity_atlas_texture),
            entity_atlas_texture,
            atlas_texture: std::sync::Arc::new(atlas_texture),
            atlas_upload_buffer: None,
            atlas_bind_group_layout,
            meshes: HashMap::default(),
            terrain_arena: TerrainArena::new(device),
            terrain_draws: IndirectDraws::new(device),
            translucent_arena: TerrainArena::new(device),
            translucent_draws: IndirectDraws::new(device),
            terrain_translucent_pipeline,
            lit_terrain_translucent_pipeline,
            daylight: 1.0,
            vanilla_game_time: 0.0,
            // Beides noetig: mehrere Aufrufe aus einem Puffer, und first_instance != 0 (daran
            // haengt der Spalten-Versatz). Auf Vulkan-Desktop-Treibern praktisch immer da.
            multi_draw: device.features().contains(wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE),
            gbuffer_terrain_pipeline,
            occlusion: OcclusionCulling::new(),
            cave_culling: true,
            cull_radius: 32,
            culling_stats: (0, 0),
            translucent_meshes: HashMap::default(),
            draw_order: Vec::new(),
            translucent_order: Vec::new(),
            order_center: None,
            order_dirty: false,
            translucent_sort_pos: None,
            column_offset_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("column offsets"),
                size: COLUMN_OFFSET_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            column_offset_capacity: 1,
            column_offset_scratch: Vec::new(),
            zero_offset_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zero column offset"),
                size: COLUMN_OFFSET_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sort_dirty: HashSet::default(),
            sort_thread: SortThread::spawn(),
            sort_in_flight_quads: 0,
            translucent_generation: 0,
            entity_buffer,
            entity_tint_buffer,
            entity_vertex_count: 0,
            entity_translucent_count: 0,
            entity_head_translucent: 0,
            outline_pipeline,
            own_mask_pipeline,
            own_vertex_count: 0,
            sharp_entity_ranges: Vec::new(),
            outline_buffer,
            outline_color_buffer,
            outline_vertex_count: 0,
            entity_pipeline,
            entity_translucent_pipeline,
            entity_light_buffer,
            entity_light_count: 0,
            lit_entity_pipeline,
            lit_entity_translucent_pipeline,
            hand_light: None,
            keep_depth: false,
            hand_lit_pipeline,
            ghost_light_buffer,
            ghost_light_count: 0,
            gbuffer_ghost_pipeline,
            hand_camera_buffer,
            hand_camera_bind_group,
            hand_buffer,
            doll_camera_buffer,
            doll_camera_bind_group,
            doll_buffer,
            doll_vertex_count: 0,
            glint_pipeline,
            glint_bind_group_layout,
            glint_bind_group,
            glint_uniform_buffer,
            glint_sampler,
            hand_glint: crate::glint::GlintBuffer::new(device, "hand glint buffer", HAND_VERTEX_CAPACITY),
            entity_glint: crate::glint::GlintBuffer::new(device, "entity glint buffer", ENTITY_GLINT_VERTEX_CAPACITY),
            doll_glint: crate::glint::GlintBuffer::new(device, "doll glint buffer", DOLL_VERTEX_CAPACITY),
            hand_vertex_count: 0,
            particle_opaque_pipeline,
            particle_translucent_pipeline,
            particle_bind_group,
            particle_buffer,
            particle_draws: ParticleDraws::default(),
            weather_pipeline,
            weather_bind_groups: None,
            weather_buffer,
            weather_draws: [0; 2],
        }
    }
}
