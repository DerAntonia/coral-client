// GpuState::new: Geraet, Oberflaeche, Pipelines, Texturen und Bind-Groups.

use super::*;

impl GpuState {
    pub(crate) fn new(
        window: Arc<Window>,
        installation: &mut assets::MinecraftInstallation,
        atlas_mips: &[Vec<u8>],
        atlas_size: u32,
        entity_atlas_rgba: &[u8],
        entity_atlas_size: u32,
        particle_atlas: &assets::ParticleAtlas,
        ui_atlas_rgba: &[u8],
        ui_atlas_size: u32,
        font: FontKind,
    ) -> Self {
        let size = window.inner_size();

        let backends = if cfg!(windows) { wgpu::Backends::DX12 } else { wgpu::Backends::VULKAN };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..Default::default() });

        let surface = instance.create_surface(window.clone()).expect("Surface konnte nicht erstellt werden");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("kein passender Vulkan-Adapter gefunden");
        let adapter_info = adapter.get_info();
        println!("[gpu] {} (Vendor 0x{:04x}, Treiber: {})", adapter_info.name, adapter_info.vendor, adapter_info.driver);

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Coral Client device"),
                // Zeitstempel fuer die GPU-Zeit je Pass im Frame-Profiler (s. gpu_timer) -- nur,
                // wenn der Adapter sie kann; kostet nichts, solange niemand misst.
                // Dazu Mehrfach-Indirekt-Zeichnen fuers Gelaende (ein Aufruf statt ~1800, s.
                // render::BlockRenderer::prepare_terrain_draws); fehlt es, zeichnet der
                // Renderer je Spalte einzeln.
                required_features: adapter.features()
                    & (wgpu::Features::TIMESTAMP_QUERY
                        | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
                        | wgpu::Features::MULTI_DRAW_INDIRECT
                        | wgpu::Features::INDIRECT_FIRST_INSTANCE),
                // Groessere Speicher-Puffer fuer die Lichtkarten des Shader-Pfads.
                required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .expect("Device konnte nicht erstellt werden");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats.iter().copied().find(wgpu::TextureFormat::is_srgb).unwrap_or(surface_caps.formats[0]);

        // Immer Immediate: kein Warten auf den VBlank, minimale Latenz. Den Riss legt die
        // Riss-Steuerung in die Austastluecke. Eine Auswahl gibt es nicht mehr. Kann der Treiber
        // kein Immediate, Mailbox (reisst nicht, wartet aber auch nicht), sonst Fifo -- den
        // garantiert Vulkan.
        let present_modes = &surface_caps.present_modes;
        let present_mode = [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox]
            .into_iter()
            .find(|m| present_modes.contains(m))
            .unwrap_or(wgpu::PresentMode::Fifo);
        println!("[gpu] Present-Modi laut Treiber: {present_modes:?}, nehme {present_mode:?}");

        // CORAL_SCREENSHOT: die Swapchain zusaetzlich lesbar machen, sonst nicht (s. screenshot.rs).
        let screenshot_supported = screenshot::enabled() && surface_caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        if screenshot::enabled() {
            println!("[screenshot] F2 kopiert das Bild in die Zwischenablage: {}", if screenshot_supported { "bereit" } else { "Swapchain nicht lesbar, aus" });
        }

        let config = wgpu::SurfaceConfiguration {
            // COPY_DST zusaetzlich zu RENDER_ATTACHMENT: der letzte Schritt jedes Frames
            // kopiert das fertige Bild aus offscreen_texture in die Swapchain-Textur (s.
            // dortigen Kommentar), statt direkt hineinzuzeichnen -- braucht Schreibzugriff
            // per Kopie.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | if screenshot_supported { wgpu::TextureUsages::COPY_SRC } else { wgpu::TextureUsages::empty() },
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };
        // configure erst am Ende, s. configure_surface.

        // Offscreen-Farbtextur, in die render_world tatsaechlich zeichnet (Welt, HUD-Text,
        // Crosshair) -- NICHT direkt in die Swapchain-Textur. Grund: das Crosshair-
        // Invertieren muss den bereits gezeichneten Bildinhalt zurücklesen (s.
        // crosshair_sample_texture), und ein Rueck-Kopieren direkt aus der praesentierbaren
        // Swapchain-Textur ist auf manchen Treibern (dieser RADV-Treiber meldet sich selbst
        // als "not a conformant Vulkan implementation") unzuverlaessig -- vermutlich
        // dieselbe Klasse Problem wie beim zuerst versuchten Blend-Trick. Eine selbst
        // verwaltete Textur mit klar definierter COPY_SRC-Nutzung umgeht das. Der letzte
        // Schritt jedes Frames kopiert das fertige Bild einmal komplett in die Swapchain-
        // Textur (s. render_world).
        let (offscreen_texture, offscreen_view) = create_offscreen_texture(&device, surface_format, config.width, config.height);

        // --- Quad-Pipeline (gefuellte, farbige Flaechen) ---
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../quad.wgsl").into()),
        });
        let quad_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad pipeline layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let quad_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad pipeline"),
            layout: Some(&quad_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    // ALPHA_BLENDING, nicht REPLACE. ECHTER BUG gewesen (bei 50 % Deckkraft
                    // sah man die Kamera gar nicht): mit REPLACE wird der Alphawert jeder
                    // UI-Flaeche schlicht ignoriert, jedes Rechteck ueberschreibt das Bild
                    // deckend. Die halbdurchsichtige Abdunklung hinter dem Inventar wurde so
                    // zu deckendem Schwarz, egal welcher Prozentwert eingestellt war.
                    //
                    // Betraf ALLE bewusst durchscheinenden Flaechen dieser Pipeline (auch die
                    // Linien, die als duenne Quads durchlaufen): Slot-Hervorhebung (0.25),
                    // Scoreboard-Hintergrund (0.4), Sterbe-Ueberlagerung (0.55), Graph-
                    // Hintergrund (0.55), Raster und Griffe im HUD-Editor (0.08/0.35/0.6).
                    // Die sahen alle deckend aus. Text- und Sprite-Pipeline hatten die
                    // Blendung von Anfang an, nur diese hier nicht.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let quad_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad vertex buffer"),
            size: (std::mem::size_of::<Vertex>() * VERTS_PER_QUAD * MAX_UI_RECTS) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Eigener Puffer fuer Liniensegmente (F3-CPU-Graph, s. render_world) -- getrennt vom
        // quad_vertex_buffer, weil ein voller Graph (16+ Kerne * 89 Segmente) die dort fuer
        // normale UI-Rechtecke vorgesehene Kapazitaet (MAX_UI_RECTS) um ein Vielfaches
        // sprengen wuerde.
        let line_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("line vertex buffer"),
            size: (std::mem::size_of::<Vertex>() * VERTS_PER_QUAD * MAX_UI_LINE_SEGMENTS) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // --- Zeiger-Pipeline: zeichnet EIN Quad, die Form entsteht im Fragment-Shader aus
        // dem Uniform (s. cursor.wgsl). Ein eigener Zeichendurchgang bleibt noetig, damit der
        // Zeiger NACH Flaechen, Sprites und Text ganz vorn liegt (s. render_menu).
        let cursor_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cursor shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../cursor.wgsl").into()),
        });
        let cursor_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cursor uniform buffer"),
            size: std::mem::size_of::<CursorUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cursor_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cursor bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let cursor_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cursor bind group"),
            layout: &cursor_bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: cursor_uniform_buffer.as_entire_binding() }],
        });
        let cursor_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cursor pipeline layout"),
            bind_group_layouts: &[&cursor_bind_group_layout],
            push_constant_ranges: &[],
        });
        let cursor_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cursor pipeline"),
            layout: Some(&cursor_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &cursor_shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &cursor_shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    // Die geglaetteten Randpixel sind halbdurchsichtig -- ohne Blendung
                    // waeren sie deckend und die Treppe waere nur bunt statt weg.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- Crosshair-Pipeline: sampelt offscreen_view DIREKT (kein Kopierschritt mehr,
        // s. crosshair_vertices-Kommentar) und gibt im Fragment-Shader die Gegenfarbe des
        // Untergrunds aus (render::CONTRAST_COLOR_WGSL, wird an den Shader angehaengt).
        // Vor der Fadenkreuz-Pipeline: die zeichnet HUD-Zahlen mit den Glyphen dieses Atlas und
        // bindet dafuer sein Layout als Gruppe 1 (s. crosshair.wgsl).
        let mut font_atlas = FontAtlas::build(&device, &queue, installation, font).expect("Minecraft-Font konnte nicht geladen werden");
        let pack_fonts = installation.load_pack_fonts();
        let crosshair_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crosshair shader"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", include_str!("../crosshair.wgsl"), render::CONTRAST_COLOR_WGSL).into()),
        });
        let crosshair_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("crosshair bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Komplementaer-Farbe ueber mehrere Frames (crosshair.wgsl::CrosshairFade).
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let crosshair_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("crosshair sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        // Farbmodus, Aufhellung, feste Farbe, Brenn-Farbe und die Flaeche unter dem Kreuz (s.
        // crosshair.wgsl::CrosshairSettings/CROSSHAIR_COLOR_MODES), 80 Byte. Wird bei jedem
        // Frame mit den aktuellen Settings-Werten ueberschrieben (render_world), keine eigene
        // Bind-Group-Neuerzeugung noetig.
        let crosshair_mode_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("crosshair settings buffer"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Zustand der Komplementaer-Farbe je Farbplatz (16 Byte, s. crosshair.wgsl::CrosshairFade),
        // nur auf der GPU gelesen und geschrieben. Anfangs Gelb.
        let crosshair_fade_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("crosshair fade buffer"),
            size: 16 * CROSSHAIR_FADE_SLOTS as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let crosshair_bind_group = create_crosshair_bind_group(
            &device,
            &crosshair_bind_group_layout,
            &offscreen_view,
            &crosshair_sampler,
            &crosshair_mode_buffer,
            &crosshair_fade_buffer,
        );
        let crosshair_fade_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("crosshair fade pipeline layout"),
            bind_group_layouts: &[&crosshair_bind_group_layout],
            push_constant_ranges: &[],
        });
        let crosshair_fade_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("crosshair fade pipeline"),
            layout: Some(&crosshair_fade_pipeline_layout),
            module: &crosshair_shader,
            entry_point: "cs_fade",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let crosshair_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("crosshair pipeline layout"),
            bind_group_layouts: &[&crosshair_bind_group_layout, &*font_atlas.bind_group_layout],
            push_constant_ranges: &[],
        });
        let crosshair_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("crosshair pipeline"),
            layout: Some(&crosshair_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &crosshair_shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CrosshairVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 6]>() as wgpu::BufferAddress,
                            shader_location: 3,
                            format: wgpu::VertexFormat::Float32,
                        },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 7]>() as wgpu::BufferAddress,
                            shader_location: 4,
                            format: wgpu::VertexFormat::Float32,
                        },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                            shader_location: 5,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &crosshair_shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                // Alpha-Blending wegen der Glyphen-Kanten (DM Sans ist kantengeglaettet) und der
                // Deckkraft der Pfeil-Zahl. Kreuz und Kontur geben Alpha 1 aus -- fuer sie aendert
                // sich nichts gegenueber dem frueheren REPLACE.
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- CMAA2 (s. cmaa2.wgsl-Kommentar) -- Compute-Infrastruktur + gather-basierte,
        // kontrastadaptive Kantenglaettung.
        let (cmaa2_output_texture, cmaa2_output_view) = create_cmaa2_output_texture(&device, config.width, config.height);
        let cmaa2_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cmaa2 compute shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../cmaa2.wgsl").into()),
        });
        let cmaa2_compute_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cmaa2 compute bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let cmaa2_compute_bind_group =
            create_cmaa2_compute_bind_group(&device, &cmaa2_compute_bind_group_layout, &offscreen_view, &cmaa2_output_view);
        let cmaa2_compute_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cmaa2 compute pipeline layout"),
            bind_group_layouts: &[&cmaa2_compute_bind_group_layout],
            push_constant_ranges: &[],
        });
        let cmaa2_compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("cmaa2 compute pipeline"),
            layout: Some(&cmaa2_compute_pipeline_layout),
            module: &cmaa2_shader,
            entry_point: "cmaa2_cs",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let cmaa2_blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cmaa2 blit shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../cmaa2_blit.wgsl").into()),
        });
        let cmaa2_blit_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cmaa2 blit bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let cmaa2_blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cmaa2 blit sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let cmaa2_blit_bind_group =
            create_cmaa2_blit_bind_group(&device, &cmaa2_blit_bind_group_layout, &cmaa2_output_view, &cmaa2_blit_sampler);
        let cmaa2_blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cmaa2 blit pipeline layout"),
            bind_group_layouts: &[&cmaa2_blit_bind_group_layout],
            push_constant_ranges: &[],
        });
        let cmaa2_blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cmaa2 blit pipeline"),
            layout: Some(&cmaa2_blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &cmaa2_blit_shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &cmaa2_blit_shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- Farb-Durchgang (Modul "Farbe", s. color.wgsl) ---
        let color_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("color shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../color.wgsl").into()),
        });
        let color_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("color bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let color_settings_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("color settings buffer"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let color_bind_group = create_color_bind_group(&device, &color_bind_group_layout, &offscreen_view, &color_settings_buffer);
        let color_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("color pipeline layout"),
            bind_group_layouts: &[&color_bind_group_layout],
            push_constant_ranges: &[],
        });
        let color_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("color pipeline"),
            layout: Some(&color_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &color_shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &color_shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Zwei Balken (Rechtecke) als "+"-Form -- 12 Vertices (2 Quads * 6), plus noch einmal
        // so viele fuer die optionale Kontur (s. crosshair_vertices). Feste Groesse.
        let crosshair_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("crosshair vertex buffer"),
            size: (std::mem::size_of::<CrosshairVertex>() * CROSSHAIR_MAX_VERTICES) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Text-Pipeline (Font-Atlas, alpha-geblendet, per-Vertex-Farbe) ---
        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("font shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../font.wgsl").into()),
        });
        let text_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("text pipeline layout"),
            bind_group_layouts: &[&*font_atlas.bind_group_layout],
            push_constant_ranges: &[],
        });
        let text_attributes = [
            wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute {
                offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];
        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text pipeline"),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &text_attributes,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    // Alpha-Blending, damit Glyphen ueber farbigen Flaechen (Buttons) sitzen,
                    // statt deren Hintergrund mit Schwarz zu ueberschreiben.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Dieselbe Schrift mit Tiefentest gegen die Welt (Schilder, s. font.wgsl::vs_world):
        // verdeckt von allem, was davor steht, auch von Entities. Tiefe des Welt-Durchgangs
        // (render::block_renderer::DEPTH_FORMAT), selbst ohne Tiefe zu schreiben.
        let world_text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world text pipeline"),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: "vs_world",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &text_attributes,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sign_text_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sign text vertex buffer"),
            size: (std::mem::size_of::<TextVertex>() * MAX_SIGN_TEXT_VERTICES) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let text_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text vertex buffer"),
            size: (std::mem::size_of::<TextVertex>() * VERTS_PER_QUAD * MAX_UI_GLYPHS) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Sprite-Pipeline (texturierte UI-Quads: GUI-Sheets, Item-Icons) ---
        let sprite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../sprite.wgsl").into()),
        });
        let sprite_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sprite_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sprite sampler"),
            // Nearest in beide Richtungen: Item-Icons werden fast immer VERGROESSERT
            // gezeichnet (16px-Sprite auf 32-64px Slot) -- linear waere ein Weichzeichner.
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let wallpaper_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("wallpaper sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sprite_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite pipeline layout"),
            bind_group_layouts: &[&sprite_bind_group_layout],
            push_constant_ranges: &[],
        });
        let sprite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite pipeline"),
            layout: Some(&sprite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sprite_shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                // Gleiches Vertex-Layout wie der Text (Position, UV, Farbe) -- s. TextVertex.
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sprite_shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // GUI-Glint: Gruppe 0 wie beim Sprite (der Icon-Atlas als Maske), Gruppe 1 Glint-
        // Textur + Sampler + Matrix. Gemischt wie in der Welt: additiv, das Ziel-Alpha bleibt.
        let glint_gui_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gui glint bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let glint_sprite_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gui glint pipeline layout"),
            bind_group_layouts: &[&sprite_bind_group_layout, &glint_gui_bind_group_layout],
            push_constant_ranges: &[],
        });
        let glint_sprite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gui glint pipeline"),
            layout: Some(&glint_sprite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sprite_shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sprite_shader,
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
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let glint_gui_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gui glint uniform"),
            size: std::mem::size_of::<render::glint::GlintUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glint_gui_sampler = render::glint::create_sampler(&device);
        let sprite_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite vertex buffer"),
            // +1: das Hintergrundbild des Startbildschirms vorneweg (s. render_menu).
            size: (std::mem::size_of::<TextVertex>() * VERTS_PER_QUAD * (MAX_UI_SPRITES + 1)) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (ui_atlas_bind_group, ui_atlas_texture) =
            create_ui_atlas_bind_group(&device, &queue, &sprite_bind_group_layout, &sprite_sampler, ui_atlas_rgba, ui_atlas_size);

        // Startet leer -- gefuellt wird ausschliesslich aus echten Server-Chunks,
        // sobald die Verbindung steht (s. App::mesh_pending_columns).
        let block_renderer = render::BlockRenderer::new(
            &device,
            &queue,
            surface_format,
            config.width,
            config.height,
            atlas_mips,
            atlas_size,
            entity_atlas_rgba,
            entity_atlas_size,
            &particle_atlas.rgba,
            particle_atlas.size,
        );
        let mut block_renderer = block_renderer;
        let glint_texturen = load_glint_textures(installation);
        set_glint_textures(&mut block_renderer, &device, &queue, &glint_texturen);
        let (glint_rgba, glint_w, glint_h) = glint_texture_part(&glint_texturen[0]);
        let glint_gui_view = render::glint::upload_texture(&device, &queue, glint_rgba, glint_w, glint_h);
        let glint_gui_bind_group =
            create_gui_glint_bind_group(&device, &glint_gui_bind_group_layout, &glint_gui_view, &glint_gui_sampler, &glint_gui_uniform);
        let block_atlas_sprite_bind_group =
            create_block_sprite_bind_group(&device, &sprite_bind_group_layout, &sprite_sampler, &block_renderer);
        let entity_atlas_sprite_bind_group =
            create_entity_sprite_bind_group(&device, &sprite_bind_group_layout, &sprite_sampler, &block_renderer);
        let gpu_timer = gpu_timer::GpuTimer::new(&device, &queue);
        println!("[gpu] Zeitstempel je Pass: {}", if gpu_timer.is_some() { "verfuegbar" } else { "nicht unterstuetzt" });

        let custom_sky = render::custom_sky::SkyRenderer::new(&device, surface_format.remove_srgb_suffix());
        let entity_outline = render::entity_outline::EntityOutline::new(&device, surface_format.remove_srgb_suffix());
        let motion_blur = render::motion_blur::MotionBlur::new(&device, surface_format);
        let gradient_sky = render::gradient_sky::GradientSky::new(&device, surface_format);
        let device = std::sync::Arc::new(device);
        let queue = std::sync::Arc::new(queue);
        let pack_glyphs = Some(Arc::new(font_atlas::PackGlyphCache::new(&device, queue.clone(), pack_fonts)));
        font_atlas.set_pack(&device, pack_glyphs.clone());
        let mut state = Self {
            instance,
            surface,
            surface_ok: false,
            surface_retry_at: Instant::now(),
            device,
            queue,
            config,
            window,
            offscreen_texture,
            offscreen_view,
            quad_pipeline,
            quad_vertex_buffer,
            line_vertex_buffer,
            cursor_pipeline,
            cursor_uniform_buffer,
            cursor_bind_group,
            crosshair_pipeline,
            crosshair_vertex_buffer,
            crosshair_bind_group_layout,
            crosshair_sampler,
            crosshair_mode_buffer,
            crosshair_fade_buffer,
            crosshair_fade_pipeline,
            crosshair_fade_clock: Instant::now(),
            crosshair_bind_group,
            cmaa2_output_texture,
            cmaa2_output_view,
            cmaa2_compute_pipeline,
            cmaa2_compute_bind_group_layout,
            cmaa2_compute_bind_group,
            cmaa2_blit_pipeline,
            cmaa2_blit_bind_group_layout,
            cmaa2_blit_sampler,
            cmaa2_blit_bind_group,
            color_pipeline,
            color_bind_group_layout,
            color_settings_buffer,
            color_bind_group,
            text_pipeline,
            text_vertex_buffer,
            world_text_pipeline,
            sign_text_buffer,
            sign_glyphs: Default::default(),
            sign_font: Default::default(),
            label_font: Default::default(),
            font_atlas,
            extra_fonts: Vec::new(),
            pending_fonts: Vec::new(),
            pack_glyphs,
            layout_font: Default::default(),
            plain_numbers: Default::default(),
            under_sprites: Default::default(),
            text_shadow_style: TextShadow::Outline,
            text_shadow_strength: 1.0,
            text_shadow_half: false,
            text_runs: Default::default(),
            text_clips: Default::default(),
            stable_numbers: Default::default(),
            world_label_glyphs: Default::default(),
            world_label_quads: Default::default(),
            sprite_pipeline,
            glint_sprite_pipeline,
            glint_gui_bind_group_layout,
            glint_gui_bind_group,
            glint_gui_uniform,
            glint_gui_sampler,
            sprite_vertex_buffer,
            sprite_bind_group_layout,
            sprite_sampler,
            ui_atlas_bind_group,
            ui_atlas_texture,
            block_atlas_sprite_bind_group,
            entity_atlas_sprite_bind_group,
            wallpaper: None,
            last_view: None,
            connect_view: None,
            offscreen_world_only: false,
            server_icons: None,
            pack_icons: None,
            logos: None,
            world_backdrop: None,
            wallpaper_sampler,
            block_renderer,
            adapter_info,
            last_submission: None,
            screenshot_supported,
            screenshot_requested: false,
            fence_wait_zero: false,
            last_sort_ms: 0.0,
            measure_gpu_done: false,
            gpu_timer,
            use_shader_path: false,
            shader_path: None,
            lod: None,
            lod_active: false,
            sky_angle: 0.0,
            custom_sky,
            entity_outline,
            motion_blur,
            motion_blur_strength: 0.0,
            motion_blur_interval_s: 1.0 / 60.0,
            sky_layers: Vec::new(),
            gradient_sky,
            sky_gradient: None,
            atmosphere: true,
            scattering: 1.0,
            sun_size_deg: 2.0,
            block_shadows: true,
            block_softness_deg: 6.0,
            lightmap_debug: 0,
            present_at_ns: None,
        };
        state.configure_surface();
        state
    }
}
