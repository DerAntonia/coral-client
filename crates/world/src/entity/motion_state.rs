// Positions-Updates, Interpolationsabstand, Gliedmassen, Kopf, Wasserkontakt, Neigung, Gleiten,
// Alter, Ruestungsstaender und Umhang.

use super::*;

impl Entity {
    // PositionInterpolator.refreshPositionAndAngles: neues Ziel, Schrittzaehler auf Anfang.
    // Ein Update, das mitten in eine laufende Angleichung faellt, setzt sie damit fort statt
    // sie zu verwerfen -- der Startpunkt ist die aktuelle (Teil-)Position.
    //
    // `pos`/`yaw`/`pitch` sind optional: ein reines Positionspaket laesst die Rotation stehen,
    // ein reines Rotationspaket die Position -- genau wie Vanillas
    // updateTrackedPositionAndAngles(Optional, Optional, Optional), das fehlende Werte aus dem
    // laufenden Ziel des Interpolators auffuellt.
    pub(crate) fn refresh(&mut self, pos: Option<(f64, f64, f64)>, yaw: Option<f32>, pitch: Option<f32>, now: Instant) {
        // ClientPlayNetworkHandler.onEntity / onEntityPositionSync: ist das Entity selbst
        // gelenkt, merkt sich der Client nur die Serverposition (tracked_pos, schon gesetzt)
        // und laesst Stelle und Drehung, wie sie sind.
        if self.driven {
            return;
        }
        // Vanillas Entity.moveOrInterpolateTo greift nur dann zum Interpolator, wenn das
        // Entity einen hat (s. has_interpolator). Alles andere springt sofort auf die neue
        // Stelle. Bei einem Pfeil ist das richtig, weil er zwischen den seltenen
        // Server-Paketen selbst fliegt (s. tick_projectile): ein Paket ist eine KORREKTUR,
        // kein Ziel, auf das man sich ueber mehrere Ticks zubewegt. Bei einem
        // interaction-Entity (Baelle auf mcpvp.club) laege die Trefferbox sonst bis zu drei
        // Ticks hinter der Stelle, die der Server kennt -- ein schneller Ball wurde dann dort
        // geschlagen, wo er gar nicht mehr war.
        // Displays haben einen eigenen Interpolator mit der Laenge teleport_duration (Vorgabe 0).
        let display_schritte = self.display.as_ref().map(|d| d.teleport_duration.max(0) as u32);
        let springt = match display_schritte {
            Some(n) => n == 0,
            None => !has_interpolator(self.entity_type),
        };
        if self.projectile.is_some() || self.item_physics || self.block_physics || springt {
            if let Some(pos) = pos {
                self.pos = pos;
                self.target_pos = pos;
                self.tracked_pos = pos;
                self.note_update_interval(now);
            }
            if let Some(yaw) = yaw {
                self.yaw_deg = yaw;
                self.target_yaw_deg = yaw;
            }
            if let Some(pitch) = pitch {
                self.pitch_deg = pitch;
                self.target_pitch_deg = pitch;
            }
            self.interp_steps = 0;
            return;
        }
        // InterpolationHandler.interpolateTo: dasselbe Ziel bei laufender Angleichung faengt sie
        // nicht von vorn an. Bei Displays zaehlt das -- ein Tor, das ueber 20 Ticks zurueckfaehrt,
        // bekommt das Ziel oft mehrmals.
        let gleiches_ziel = self.interp_steps > 0
            && pos.is_none_or(|p| p == self.target_pos)
            && yaw.is_none_or(|y| y == self.target_yaw_deg)
            && pitch.is_none_or(|p| p == self.target_pitch_deg);
        if let Some(pos) = pos {
            self.target_pos = pos;
            self.note_update_interval(now);
        }
        if let Some(yaw) = yaw {
            self.target_yaw_deg = yaw;
        }
        if let Some(pitch) = pitch {
            self.target_pitch_deg = pitch;
        }
        match display_schritte {
            Some(_) if gleiches_ziel => {}
            Some(n) => self.interp_steps = n,
            None => self.interp_steps = INTERPOLATION_STEPS,
        }
    }

    // Reine Diagnose (F3): Abstand zwischen zwei Positions-Updates mitschreiben.
    pub(crate) fn note_update_interval(&mut self, now: Instant) {
        if self.has_moved {
            if self.update_intervals.len() == INTERVAL_SAMPLES {
                self.update_intervals.pop_front();
            }
            self.update_intervals.push_back(now.duration_since(self.updated_at).as_secs_f32() * 1000.0);
        }
        self.has_moved = true;
        self.updated_at = now;
    }

    // Diagnose: (letzter, mittlerer, groesster) Abstand zwischen zwei Positions-Updates in
    // Millisekunden. None, solange noch kein Abstand gemessen werden konnte (Entity gerade
    // erst gespawnt oder steht still -- ein stehendes Entity sendet keine Updates).
    pub fn update_interval_ms(&self) -> Option<(f32, f32, f32)> {
        if self.update_intervals.is_empty() {
            return None;
        }
        let last = *self.update_intervals.back()?;
        let sum: f32 = self.update_intervals.iter().sum();
        let avg = sum / self.update_intervals.len() as f32;
        let max = self.update_intervals.iter().copied().fold(f32::MIN, f32::max);
        Some((last, avg, max))
    }

    // Diagnose: wie alt die zuletzt empfangene Serverposition gerade ist. Das ist die
    // Verzoegerung, die allein durch das Warten auf das naechste Paket entsteht -- OHNE die
    // Laufzeit im Netz davor, die wir client-seitig nicht messen koennen (s. F3-Overlay).
    #[must_use] 
    pub fn position_age_ms(&self, now: Instant) -> f32 {
        now.duration_since(self.updated_at).as_secs_f32() * 1000.0
    }

    // Diagnose: die zuletzt vom Server empfangene Position (das Ziel der Angleichung).
    #[must_use] 
    pub const fn target_pos(&self) -> (f64, f64, f64) {
        self.target_pos
    }

    // Laufende Schrittphase und deren Amplitude fuer die Modell-Pose. `tick_progress` ist der
    // Bruchteil des laufenden Ticks (0..1) -- damit gleitet die Schwingung zwischen den Ticks
    // statt 20 mal pro Sekunde weiterzuspringen.
    #[must_use] 
    pub fn limb_swing(&self, tick_progress: f32) -> (f32, f32) {
        (self.limbs.animation_progress(tick_progress), self.limbs.amplitude(tick_progress))
    }

    // Der laufende Handschlag mit Zwischentick-Anteil und der Arm, der schlaegt
    // (LivingEntity.getHandSwingProgress). Der Sprung von 1 zurueck auf 0 am Ende des Schwungs
    // wird dabei UEBERSPRUNGEN: Vanilla addiert bei negativer Differenz eine 1 dazu, sonst
    // liefe der Arm im letzten Zwischenschritt rueckwaerts durch die ganze Bewegung.
    #[must_use] 
    pub fn hand_swing(&self, tick_progress: f32) -> (f32, bool) {
        let mut delta = self.hand_swing_progress - self.last_hand_swing_progress;
        if delta < 0.0 {
            delta += 1.0;
        }
        // Der Arm der geschwungenen Hand (s. left_handed).
        (self.last_hand_swing_progress + delta * tick_progress.clamp(0.0, 1.0), self.swing_off_hand == self.left_handed)
    }

    // Set Head Rotation: Ziel setzen, Schrittzaehler auf Anfang (Vanilla:
    // updateTrackedHeadRotation(yaw, 3)).
    pub const fn set_head_yaw(&mut self, head_yaw_deg: f32) {
        self.server_head_yaw_deg = head_yaw_deg;
        self.head_steps = HEAD_YAW_STEPS;
    }

    // Kopfdrehung RELATIV zum Koerper -- der Wert, den die Modell-Pose fuer den Kopf braucht
    // (Vanilla: LivingEntityRenderer.updateRenderState, wrapDegrees(headYaw - bodyYaw)).
    //
    // Beide Winkel mit DEMSELBEN Zwischentick-Anteil: interpolierte man nur einen von beiden,
    // zappelte ihre Differenz -- und die Differenz ist genau das, was hier herauskommt.
    #[must_use] 
    pub fn relative_head_yaw(&self, tick_progress: f32) -> f32 {
        let head = lerp_angle(tick_progress.clamp(0.0, 1.0), self.last_head_yaw_deg, self.head_yaw_deg);
        crate::entity_model::wrap_degrees(head - self.render_yaw(tick_progress))
    }

    // Beruehrt die Trefferbox Wasser? Eine Naeherung an Vanillas updateInWaterState: die Ecken
    // und die Mitte der Box in Fuss- und Kopfhoehe gegen die Bloecke dort. Die Fuellhoehe
    // fliessenden Wassers bleibt aussen vor -- fuer die Schwimmhaltung reicht "da ist Wasser".
    pub(crate) fn touches_water(&self, world: &crate::chunk::World) -> bool {
        if !is_living(self.entity_type) {
            return false;
        }
        let (w, h) = self.box_size();
        let r = w / 2.0 - 0.001;
        let (x, y, z) = self.pos;
        for dy in [0.001, h * 0.5, h - 0.001] {
            for (dx, dz) in [(0.0, 0.0), (-r, -r), (r, -r), (-r, r), (r, r)] {
                let state = world.block_at((x + dx).floor() as i32, (y + dy).floor() as i32, (z + dz).floor() as i32);
                if crate::fluid::fluid_kind(state) == Some(crate::fluid::FluidKind::Water) {
                    return true;
                }
            }
        }
        false
    }

    /// Schwimm-/Kriechhaltung fuers Bild (s. `entity_model::Lean`) -- Vanilla
    /// AvatarRenderer.setupRotations, Zweig swimAmount > 0.
    #[must_use] 
    pub fn lean(&self, tick_progress: f32) -> crate::entity_model::Lean {
        let t = tick_progress.clamp(0.0, 1.0);
        crate::entity_model::Lean {
            amount: self.last_swim_amount + (self.swim_amount - self.last_swim_amount) * t,
            pitch_deg: self.render_pitch(t),
            in_water: self.in_water,
            swimming: self.pose == POSE_SWIMMING,
        }
    }

    // Gleitflug fuers Bild (s. entity_model::Glide), None am Boden. Die Geschwindigkeit ist die
    // Bewegung des letzten Ticks -- fremde Spieler bewegen sich bei uns nur ueber ihre
    // Positionspakete, eine eigene Geschwindigkeit schickt der Server fuer sie nicht mit.
    #[must_use] 
    pub fn glide(&self, tick_progress: f32) -> Option<crate::entity_model::Glide> {
        if !self.gliding || !is_living(self.entity_type) {
            return None;
        }
        let t = tick_progress.clamp(0.0, 1.0);
        Some(crate::entity_model::Glide {
            ticks: self.gliding_ticks as f32 + t,
            pitch_deg: self.render_pitch(t),
            yaw_deg: lerp_angle(t, self.last_yaw_deg, self.yaw_deg),
            velocity: [
                (self.pos.0 - self.last_pos.0) as f32,
                (self.pos.1 - self.last_pos.1) as f32,
                (self.pos.2 - self.last_pos.2) as f32,
            ],
        })
    }

    // Alter fuer die Ruhe-Bewegung, mit Zwischentick-Anteil.
    #[must_use] 
    pub fn age_ticks(&self, tick_progress: f32) -> f32 {
        self.age_ticks + tick_progress
    }

    // Hoehe eines Armorstandes nach seinen Flags (ArmorStandEntity: MARKER_DIMENSIONS =
    // 0x0, SMALL_DIMENSIONS = die halbe Normalgroesse, sonst EntityType.ARMOR_STAND mit
    // 0.5 x 1.975). Nur fuer den Ankerpunkt des Namensschildes gedacht -- der Aufrufer prueft
    // vorher, dass es sich ueberhaupt um einen Armorstand handelt.
    #[must_use] 
    pub const fn armor_stand_flags(&self) -> u8 {
        self.armor_stand_flags
    }

    #[must_use] 
    pub fn armor_stand_height(&self) -> f64 {
        const ARMOR_STAND_HEIGHT: f64 = 1.975;
        if self.armor_stand_flags & protocol::packets::ARMOR_STAND_FLAG_MARKER != 0 {
            0.0
        } else if self.armor_stand_flags & protocol::packets::ARMOR_STAND_FLAG_SMALL != 0 {
            ARMOR_STAND_HEIGHT * 0.5
        } else {
            ARMOR_STAND_HEIGHT
        }
    }

    // Die drei Umhang-Winkel dieses Entities (s. entity_model::cape_angles). Vanilla mischt
    // dort zwischen lastX und getX() -- genau die beiden Werte, die es hier jetzt auch gibt
    // (frueher wurde zweimal dieselbe interpolierte Position uebergeben, weil es keinen
    // Tick-Vorgaenger gab).
    #[must_use] 
    pub fn cape_angles(&self, tick_progress: f32) -> (f32, f32, f32) {
        crate::entity_model::cape_angles(
            &self.cape,
            [self.pos.0, self.pos.1, self.pos.2],
            [self.last_pos.0, self.last_pos.1, self.last_pos.2],
            self.render_yaw(tick_progress),
            tick_progress,
        )
    }
}
