// Perspektive, Schulterkamera, Freelook und Zielen.

use super::*;

impl App {
    // Kameraversatz der aktuellen Perspektive (Vanillas Camera.update()-Zweig fuer
    // thirdPerson): in der Frontansicht erst die Blickrichtung umdrehen, dann in BEIDEN
    // 3.-Person-Ansichten um THIRD_PERSON_DISTANCE nach hinten weg -- weshalb die Kamera
    // vorn vor dem Spieler landet und ihn anschaut. Die Distanz wird an Waenden verkuerzt
    // (s. physics::clip_camera_distance), sonst steckte man in der Wand und saehe ins Leere.
    pub(crate) fn update_perspective_offset(&mut self) {
        self.update_camera_look();
        self.camera.roll_degrees = self.hurt_cam_roll();
        self.camera.view_inverted = self.perspective.is_front_view();
        self.own_player_alpha = 1.0;
        // Schulterwechsel: die Kamera gleitet in gut einer Zehntelsekunde hinueber.
        let (seite, seit) = self.shoulder_side_shown;
        let jetzt = Instant::now();
        let k = 1.0 - (-jetzt.duration_since(seit).as_secs_f32() / 0.06).exp();
        self.shoulder_side_shown = (seite + (self.shoulder_side - seite) * k, jetzt);
        if self.perspective.is_first_person() {
            self.camera.view_offset = glam::Vec3::ZERO;
            return;
        }
        // Hoehenversatz ZUERST auf das Auge legen: das Zurueckweichen (und damit auch die
        // Wandpruefung) soll von der tatsaechlichen Kameraposition ausgehen, nicht von der
        // Augenhoehe -- sonst rutschte eine angehobene Kamera hinterher doch noch in die
        // Decke, weil der Strahl eine Etage tiefer geprueft hat.
        let y_offset = f64::from(self.settings.camera_y_offset_of(self.perspective));
        let eye = self.camera.position + DVec3::new(0.0, y_offset, 0.0);
        // Schulterkamera: erst zur Seite (an Waenden ebenso gekappt), von dort nach hinten.
        let mut drehpunkt = eye;
        if self.shoulder_active() {
            let seitlich = f64::from(self.settings.shoulder_offset_cm) / 100.0 * f64::from(self.shoulder_side_shown.0);
            if seitlich.abs() > 1e-3 {
                let rechts = self.camera.view_right().as_dvec3() * seitlich.signum();
                drehpunkt = eye + rechts * physics::clip_camera_distance(eye, rechts, seitlich.abs(), &self.world, &self.block_shapes);
            }
        }
        let back = -self.camera.view_forward().as_dvec3();
        let wanted = f64::from(self.settings.camera_distance_of(self.perspective));
        let distance = physics::clip_camera_distance(drehpunkt, back, wanted, &self.world, &self.block_shapes);
        let kamera = drehpunkt + back * distance;
        self.camera.view_offset = (kamera - self.camera.position).as_vec3();
        if self.shoulder_active() && self.settings.shoulder_fade {
            let abstand = kamera.distance(self.camera.position) as f32;
            self.own_player_alpha = ((abstand - SHOULDER_FADE_NEAR) / (SHOULDER_FADE_FAR - SHOULDER_FADE_NEAR)).clamp(0.0, 1.0);
        }
    }

    // Schulterkamera: eingeschaltet und in der 3. Person von hinten.
    pub(crate) fn shoulder_active(&self) -> bool {
        self.settings.shoulder_cam && self.perspective == Perspective::ThirdPersonBack
    }

    // Kamera frei vom Koerper (Schulterkamera), ausser solange Freelook laeuft.
    pub(crate) fn camera_decoupled(&self) -> bool {
        self.shoulder_active() && self.settings.shoulder_decoupled && !self.freelook_held
    }

    // Zielt der Spieler gerade mit der Kamera (Kamera frei vom Koerper)? Beim Abbauen, beim
    // gehaltenen Benutzen und kurz nach jedem Angriff oder Benutzen (SHOULDER_AIM_HOLD).
    pub(crate) fn camera_aiming(&self) -> bool {
        self.use_held || self.keybind_physical_down[KeyBindAction::Attack.index()] || self.aim_until.is_some_and(|t| Instant::now() < t)
    }

    // Je Bild: die Kamerarichtung loesen oder wieder an den Spieler binden. Wird die
    // Schulterkamera wieder fest (andere Ansicht, Einstellung aus), schaut der Spieler dorthin,
    // wohin zuletzt die Kamera sah. Waehrend des Zielens folgt der Spieler dem Kameraziel.
    pub(crate) fn update_camera_look(&mut self) {
        if !self.freelook_held {
            match (self.camera_decoupled(), self.cam_look) {
                (true, None) => self.cam_look = Some((self.camera.yaw, self.camera.pitch)),
                (false, Some((yaw, pitch))) => {
                    self.camera.yaw = yaw;
                    self.camera.pitch = pitch;
                    self.cam_look = None;
                }
                _ => {}
            }
        }
        self.camera.view_look = self.cam_look;
        if self.camera_decoupled() && self.camera_aiming() && self.mode == AppMode::World && !self.screen_overlay_open() {
            self.aim_player_at_camera_target();
        }
    }

    // Freelook-Taste: an -- Ansicht wechseln (s. Settings::freelook_view) und die Kamera vom
    // Spieler loesen; aus -- Ansicht und Kamerarichtung wie vorher.
    pub(crate) fn set_freelook(&mut self, active: bool) {
        if active == self.freelook_held {
            return;
        }
        if active {
            if !self.settings.module_freelook {
                return;
            }
            let ziel = match self.settings.freelook_view {
                FREELOOK_VIEW_FIRST => Perspective::FirstPerson,
                FREELOOK_VIEW_BACK => Perspective::ThirdPersonBack,
                FREELOOK_VIEW_FRONT => Perspective::ThirdPersonFront,
                _ => self.perspective,
            };
            // Die Kamera schaut erst dorthin, wohin sie in der neuen Ansicht ohnehin schaute.
            let blick = match (ziel, self.cam_look) {
                (Perspective::ThirdPersonFront, _) => (self.camera.yaw + 180.0, -self.camera.pitch),
                (_, Some(blick)) => blick,
                _ => (self.camera.yaw, self.camera.pitch),
            };
            self.freelook_before = Some((self.perspective, self.cam_look));
            self.freelook_held = true;
            self.perspective = ziel;
            self.cam_look = Some(blick);
        } else {
            self.freelook_held = false;
            if let Some((ansicht, blick)) = self.freelook_before.take() {
                self.perspective = ansicht;
                self.cam_look = blick;
            }
        }
        self.camera.view_look = self.cam_look;
    }

    // Wo ein Strahl ab `von` in Richtung `dir` zuerst etwas trifft (Block oder Entity), sonst
    // sein Ende nach AIM_RAY_LENGTH Bloecken.
    pub(crate) fn aim_point(&self, von: DVec3, dir: DVec3) -> DVec3 {
        const AIM_RAY_LENGTH: f64 = 128.0;
        let block = physics::raycast_block_far(von, dir, AIM_RAY_LENGTH, &self.world, &self.block_shapes)
            .map(|hit| DVec3::new(f64::from(hit.block.0) + f64::from(hit.cursor.0), f64::from(hit.block.1) + f64::from(hit.cursor.1), f64::from(hit.block.2) + f64::from(hit.cursor.2)));
        let weit = block.map_or(AIM_RAY_LENGTH, |p| p.distance(von));
        match self.entities.pick_hit((von.x, von.y, von.z), (dir.x, dir.y, dir.z), weit) {
            Some((_, t)) => von + dir * t,
            None => block.unwrap_or(von + dir * AIM_RAY_LENGTH),
        }
    }

    // Den Spieler auf den Punkt drehen, den die Kamera in der Bildmitte sieht.
    pub(crate) fn aim_player_at_camera_target(&mut self) {
        let ziel = self.aim_point(self.camera.view_position(), self.camera.view_forward().as_dvec3());
        let d = ziel - self.physics.eye_pos();
        if d.length_squared() < 1e-6 {
            return;
        }
        let d = d.normalize();
        let yaw = (-d.x).atan2(d.z).to_degrees() as f32;
        // Auf dem kuerzesten Weg: die Gier waechst sonst ueber Umdrehungen hinweg nicht mit,
        // und der Koerper drehte sich einmal ganz herum.
        self.camera.yaw += ((yaw - self.camera.yaw + 180.0).rem_euclid(360.0)) - 180.0;
        self.camera.pitch = (d.y.asin().to_degrees() as f32).clamp(-MAX_PITCH, MAX_PITCH);
    }

    // Schulterkamera, Zielendes Fadenkreuz: wo auf dem Bildschirm (0..1) der Punkt liegt, auf
    // den der Spieler zielt. None: Bildmitte (Kamera frei vom Koerper zielt selbst dorthin)
    // oder der Punkt liegt hinter der Kamera.
    pub(crate) fn shoulder_crosshair_pos(&self) -> Option<(f32, f32)> {
        if !self.shoulder_active() || !self.settings.shoulder_crosshair || self.camera_decoupled() {
            return None;
        }
        let gpu = self.gpu.as_ref()?;
        let (sw, sh) = gpu.size();
        // Das Auge dieses Bildes (camera.position, zwischen den Ticks interpoliert), nicht
        // physics.eye_pos(): das steht nur je Tick fest, und das Kreuz sprang im Lauf 20-mal pro
        // Sekunde mit.
        let eye = self.camera.position;
        let punkt = self.aim_point(eye, self.camera.forward().as_dvec3());
        let rel = (punkt - self.camera.position).as_vec3();
        let clip = self.camera.view_proj_matrix(sw / sh.max(1.0)) * glam::Vec4::new(rel.x, rel.y, rel.z, 1.0);
        if clip.w <= 1e-4 {
            return None;
        }
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        Some((f32::midpoint(x, 1.0).clamp(0.0, 1.0), ((1.0 - y) / 2.0).clamp(0.0, 1.0)))
    }
}
