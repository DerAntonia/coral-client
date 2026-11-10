// Effekt-Debug, Himmel und Wetter.

use super::*;

impl App {
    // Die Ego-Hand: der eigene rechte Arm im Sichtraum, mit dem eigenen Skin.
    //
    // Vanillas Kette (HeldItemRenderer.renderItem -> renderArmHoldingItem), Schritt fuer
    // Schritt:
    //   1. Nachschwingen: die Hand dreht sich um ein Zehntel der Differenz zwischen echter und
    //      geglaetteter Blickrichtung (s. own_render_yaw) -- beim schnellen Umsehen bleibt sie
    //      kurz zurueck.
    //   2. Platzierung samt Schlag- und Wechsel-Animation (s. world::first_person_arm_transform).
    //   3. Der Arm selbst in Grundhaltung mit leichtem Rollen nach aussen.
    // Eine Zeile fuers F3-Overlay: welche Effekte der Client dem eigenen Spieler zuschreibt und
    // was daraus fuers Tempo folgt.
    pub(crate) fn effect_debug_line(&self) -> String {
        if self.own_effects.is_empty() {
            return "Effekte: keine  |  Tempo x1.00".to_string();
        }
        let mut teile: Vec<String> = self
            .own_effects
            .iter()
            .map(|(effekt, staerke)| format!("{} {}", protocol::packets::mob_effect_name(*effekt), staerke + 1))
            .collect();
        teile.sort();
        format!("Effekte: {}  |  Tempo x{:.2}", teile.join(", "), self.effect_speed_factor())
    }

    // Der Tempofaktor aus Schnelligkeit und Langsamkeit (s. physics::effect_speed_factor).
    pub(crate) fn effect_speed_factor(&self) -> f64 {
        let staerke = |effekt: i32| self.own_effects.get(&effekt).copied();
        physics::effect_speed_factor(staerke(protocol::packets::MOB_EFFECT_SPEED), staerke(protocol::packets::MOB_EFFECT_SLOWNESS))
    }

    // Die Himmelsfarbe dieses Frames, fertig fuer wgpu (also LINEAR).
    //
    // Zwei Quellen: entweder die feste Farbe des Moduls (s. ModuleId::SkyColor) oder der
    // Tag/Nacht-Verlauf aus der Weltzeit. Die Umrechnung nach linear passiert bewusst ganz am
    // Ende und an EINER Stelle -- beide Wege liefern Anzeige-Farben.
    // Modul Himmelsfarbe: gilt nur im PvP-Pfad. Der Shader-Pfad hat eigenen Himmel, Sonne und
    // Tageslicht -- eine feste Farbe dort passte weder zum Verlauf noch zu den Schatten.
    pub(crate) fn sky_color_module_active(&self) -> bool {
        self.sky_module_mode() == Some(SkyMode::Solid)
    }

    // Die Art des Moduls Himmel, solange es wirkt (an, und nur im Leistungs-Pfad). Nicht beim
    // Launcher-Bild: das zeigt die Szene bei ihrer eigenen Uhrzeit (Tag), nicht den Himmel der Config.
    pub(crate) fn sky_module_mode(&self) -> Option<SkyMode> {
        (self.settings.module_sky_color_enabled && !crate::shader_path_on() && self.hero_fov.is_none()).then_some(self.settings.sky_mode)
    }

    // Die Weltzeit, nach der der Himmel aussieht: die echte, oder mit dem Modul (Feste
    // Uhrzeit) die gewaehlte Uhrzeit am selben Tag -- der Tag bleibt, damit Packs mit
    // Tageszyklus (days/daysLoop) ihren Himmel des Tages zeigen.
    pub(crate) fn sky_day_time(&self) -> i64 {
        // Einfarbig kennt keine Uhrzeit (die Zeile ist dort ausgegraut).
        if self.settings.module_sky_color_enabled && self.settings.module_sky_fixed_time && self.settings.sky_mode != SkyMode::Solid && self.hero_fov.is_none() {
            self.world_time_of_day.div_euclid(i64::from(SKY_TIME_DAY)) * i64::from(SKY_TIME_DAY) + i64::from(self.settings.module_sky_time)
        } else {
            self.world_time_of_day
        }
    }

    // Pack-Himmel, einmal pro Spieltick: Hoehe und Biom der Kamera (s. custom_sky::tick).
    pub(crate) fn tick_custom_sky(&mut self) {
        let Some(welt) = custom_sky::world_index(&self.dimension_type_name) else { return };
        if self.custom_sky.is_empty() {
            return;
        }
        let p = self.physics.pos;
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let biom = self.biome_names.get(self.world.biome_at(x, y, z) as usize).map_or("", String::as_str);
        self.custom_sky.tick(welt, y, biom);
    }

    // Das Wetter fuers Bild: Regenstaerke, Gewitterstaerke (wie Vanillas getThunderLevel schon
    // mit der Regenstaerke multipliziert) und die vom Modul fest vorgegebene Art.
    pub(crate) fn visual_weather(&self) -> (f32, f32, Option<precipitation::Kind>) {
        let server = (self.rain_gradient, self.thunder_gradient * self.rain_gradient, None);
        if !self.settings.module_weather {
            return server;
        }
        match self.settings.weather_mode {
            WeatherMode::Server => server,
            WeatherMode::Clear => (0.0, 0.0, None),
            WeatherMode::Rain => (1.0, 0.0, Some(precipitation::Kind::Rain)),
            WeatherMode::Snow => (1.0, 0.0, Some(precipitation::Kind::Snow)),
        }
    }

    // Das Wetter fuer den Himmel (Farbe, Pack-Himmel). Das Modul Wetter legt nur fest, was
    // faellt: fester Regen oder Schnee soll den Himmel nicht grau machen und die Ebenen des
    // Pack-Himmels nicht ausblenden. Nur "Klar" haelt auch den Himmel klar.
    pub(crate) fn sky_weather(&self) -> (f32, f32) {
        if self.settings.module_weather && self.settings.weather_mode == WeatherMode::Clear {
            return (0.0, 0.0);
        }
        (self.rain_gradient, self.thunder_gradient * self.rain_gradient)
    }

    // Wo Niederschlag faellt: Kugeln um die Kamera. Mit Modul die eingestellten Radien, ohne
    // Regen DISTANCE_DEFAULT und Schnee SNOW_RADIUS bis SNOW_RADIUS_OUTER.
    pub(crate) fn weather_bounds(&self) -> precipitation::Bounds {
        let set = &self.settings;
        if set.module_weather {
            precipitation::Bounds { rain: set.weather_radius, snow: set.weather_snow_radius, snow_max: set.weather_snow_radius_max.max(set.weather_snow_radius) }
        } else {
            precipitation::Bounds { rain: precipitation::DISTANCE_DEFAULT, snow: precipitation::SNOW_RADIUS, snow_max: precipitation::SNOW_RADIUS_OUTER }
        }
    }

    // Niederschlag, einmal pro Tick: die Daecher um die Kamera vermessen und die Teilchen
    // bewegen und erzeugen (s. precipitation). Bewusst anders als Vanilla: Barrieren halten ihn
    // nicht auf -- Server ziehen damit unsichtbare Decken und Waende ein, und Schnee, der in der
    // Luft an einer unsichtbaren Flaeche endet, sieht kaputt aus (Nutzerwunsch). Nur fuers Bild;
    // der Dreizack fragt weiter Vanillas Heightmap (s. touching_water_or_rain).
    pub(crate) fn tick_weather(&mut self) {
        let (regen, gewitter, fest) = self.visual_weather();
        let grenzen = self.weather_bounds();
        let (welt, formen, klima, palette) = (&self.world, &self.block_shapes, &self.biome_climate, &self.biome_palette);
        let art = |x: i32, y: i32, z: i32| {
            fest.or_else(|| match world::weather::precipitation(*klima.get(welt.biome_at(x, y, z) as usize)?, y) {
                world::weather::Precipitation::None => None,
                world::weather::Precipitation::Rain => Some(precipitation::Kind::Rain),
                world::weather::Precipitation::Snow => Some(precipitation::Kind::Snow),
            })
        };
        // Regenfarbe: hell, mit einem Hauch der Wasserfarbe des Bioms.
        let farbe = |x: i32, y: i32, z: i32| {
            let wasser = palette.colors_of(welt.biome_at(x, y, z)).water;
            std::array::from_fn(|i| 1.0 + (wasser[i] - 1.0) * 0.15)
        };
        self.precipitation.tick(self.camera.position, grenzen, self.settings.weather_density, self.settings.weather_snow_speed, regen, gewitter, art, farbe, |x, z| {
            welt.motion_blocking_top_except(x, z, formen, |state| world::block_registry::block_name(state) == Some("barrier"))
        }, |x, y, z| {
            // Was Flocken aufhaelt (s. weather::snow_stop).
            world::weather::snow_stop(welt.block_at(x, y, z))
        }, |x, z| welt.snow_top(x, z), WorldSnowChanges(welt));
    }

    // Niederschlag, einmal pro Frame: die Quads der Teilchen um die aktuelle Kamera.
    pub(crate) fn build_weather_vertices(&mut self) -> [u32; 2] {
        let grenzen = self.weather_bounds();
        let rechts = self.camera.right();
        let oben = rechts.cross(self.camera.view_forward());
        let mut out = std::mem::take(&mut self.weather_vertices);
        let anzahl = self.precipitation.build(
            self.camera.position,
            self.tick_progress(),
            self.physics.vel.as_vec3(),
            rechts,
            oben,
            grenzen,
            1.0,
            self.weather_layouts,
            &mut out,
        );
        self.weather_vertices = out;
        anzahl
    }

    // Pack-Himmel, einmal pro Frame: fertige Bilder an die GPU, lange nicht gebrauchte weg,
    // die Ebenen dieses Frames bestimmen und fehlende Bilder anfordern.
    pub(crate) fn update_custom_sky(&mut self) {
        let day_time = self.sky_day_time();
        let (rain, thunder) = self.sky_weather();
        // Modul Himmel: Einfarbig und Verlauf ersetzen den Pack-Himmel.
        let modus = self.sky_module_mode();
        let pack_himmel = !matches!(modus, Some(SkyMode::Solid | SkyMode::Gradient));
        let verlauf = (modus == Some(SkyMode::Gradient)).then(|| {
            let c = self.sky_clear_color();
            render::gradient_sky::GradientFrame {
                sky: [c.r as f32, c.g as f32, c.b as f32],
                clear: 1.0 - rain.clamp(0.0, 1.0),
                sky_angle: world::sky::sky_angle(day_time),
                sun: self.settings.sky_sun,
                moon: self.settings.sky_moon,
                stars: self.settings.sky_stars,
            }
        });
        let Some(gpu) = self.gpu.as_mut() else { return };
        gpu.sky_gradient = verlauf;
        let frei = self.custom_sky.poll(&mut |bild| gpu.custom_sky.add_texture(&gpu.device, bild.width, bild.height, bild.rgba));
        for id in frei {
            gpu.custom_sky.remove_texture(id);
        }
        gpu.custom_sky.upload_step(&gpu.queue);
        let max_dim = gpu.device.limits().max_texture_dimension_2d;
        // Beim Verbinden, bis die Uhrzeit da ist, und nach einem Dimensionswechsel: die Welt
        // vorladen, in der man ist (vor dem Login die zuletzt bekannte, meist die Oberwelt),
        // noch nichts zeigen. Der Login kommt vor der Uhrzeit -- bis dahin sind die Bilder
        // meist schon fertig, und der Himmel steht beim Betreten gleich richtig da. Nach einem
        // Wechsel wartet er hoechstens SKY_WORLD_WAIT, damit ein riesiges Pack nicht ewig
        // ohne Himmel bleibt.
        let verbunden = self.conn_status != ConnStatus::Disconnected;
        if !verbunden || !pack_himmel {
            self.sky_world_pending = None;
        }
        let mut wartet = self.sky_time_pending && verbunden;
        if (wartet || self.sky_world_pending.is_some()) && pack_himmel {
            let welt = custom_sky::world_index(&self.dimension_type_name).unwrap_or(0);
            let installation = &mut self.installation;
            let fertig = self.custom_sky.preload(welt, max_dim, &mut |pfad| installation.load_bytes(pfad).ok());
            if fertig || self.sky_world_pending.is_some_and(|t| t.elapsed() > SKY_WORLD_WAIT) {
                self.sky_world_pending = None;
            }
        }
        wartet |= self.sky_world_pending.is_some();
        gpu.sky_layers = match custom_sky::world_index(&self.dimension_type_name) {
            Some(welt) if pack_himmel && !wartet && !self.custom_sky.is_empty() => {
                let frame = custom_sky::SkyFrame {
                    world: welt,
                    day_time,
                    celestial: world::sky::sky_angle(day_time),
                    rain,
                    thunder,
                };
                let installation = &mut self.installation;
                self.custom_sky.frame(&frame, max_dim, &mut |pfad| installation.load_bytes(pfad).ok())
            }
            _ => Vec::new(),
        };
    }

    // Die Himmelsfarbe in sRGB (s. sky_clear_color).
    pub(crate) fn sky_srgb(&self) -> [f32; 3] {
        if self.sky_color_module_active() {
            self.settings.sky_color_rgb()
        } else {
            let (regen, gewitter) = self.sky_weather();
            world::sky::weather_sky(world::sky::time_of_day_sky(SKY_DAY_SRGB, self.sky_day_time()), regen, gewitter)
        }
    }

    pub(crate) fn sky_clear_color(&self) -> wgpu::Color {
        let srgb = self.sky_srgb();
        wgpu::Color {
            r: f64::from(world::sky::srgb_to_linear(srgb[0])),
            g: f64::from(world::sky::srgb_to_linear(srgb[1])),
            b: f64::from(world::sky::srgb_to_linear(srgb[2])),
            a: 1.0,
        }
    }
}

// Was sich fuer die Schneekarte in der Welt geaendert hat (s. precipitation::SnowChanges).
struct WorldSnowChanges<'a>(&'a world::World);

impl precipitation::SnowChanges for WorldSnowChanges<'_> {
    fn revision(&self, cx: i32, cz: i32) -> u64 {
        self.0.snow_revision(cx, cz)
    }

    fn since(&self, cx: i32, cz: i32, stand: u64) -> Option<Vec<(i32, i32, i32)>> {
        self.0.snow_changes_since(cx, cz, stand)
    }
}
