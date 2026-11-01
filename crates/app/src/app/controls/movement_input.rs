// Feuerwerk-Boost, Zeiger-Effekte, Zoom, Zeiger bewegen, Snap-Tap und Bewegungs-Flags.

use super::*;

impl App {
    // Eine Benachrichtigung oben rechts zeigen (s. notifications). Der Weg fuer alles im
    // Client, was dem Spieler etwas mitteilen will, ohne dafuer den Chat zu benutzen.
    // Raketen am eigenen Spieler (Elytra-Schub). In Vanilla tickt die Rakete als eigenes
    // Entity NACH dem Spieler (der steht vorn in der Entity-Liste) und schiebt dabei dessen
    // Geschwindigkeit an -- das wirkt also erst auf die Bewegung des naechsten Ticks. Solange
    // der Server die Rakete nicht entfernt, schiebt sie weiter; mehrere schieben nacheinander.
    pub(crate) fn tick_firework_boost(&mut self) {
        let Some(id) = self.own_entity_id else { return };
        if !self.physics.gliding {
            return;
        }
        let raketen = self
            .entities
            .iter()
            .filter(|(_, e)| e.firework_shooter == Some(id) && world::entity::entity_type_name(e.entity_type) == Some("firework_rocket"))
            .count();
        let rotation = physics::rotation_vector(self.look_pitch_deg(), self.camera.yaw);
        for _ in 0..raketen {
            self.physics.vel = physics::firework_boost(self.physics.vel, rotation);
        }
    }

    // Die Zeiger-Effekte, wie sie in diesem Augenblick aussehen.
    pub(crate) fn cursor_fx_frame(&self) -> CursorFxFrame {
        let jetzt = Instant::now();
        CursorFxFrame {
            // Nur die einstellbare Groesse (Settings::cursor_size). Das Zusammenziehen beim
            // Druecken (cursor_fx.scale) ist bewusst NICHT mehr drin -- so gewuenscht.
            // cursor_polygons skaliert um die Spitze, der Klickpunkt bleibt also stehen.
            scale: self.settings.cursor_size as f32 / 100.0,
            rotation_deg: self.cursor_fx.rotation_deg(jetzt),
            additive: self.cursor_fx.additive_alpha(jetzt),
            right: self.cursor_fx.tint(jetzt),
            circle: self.settings.cursor_circle,
            width: self.settings.cursor_width as f32 / 100.0,
        }
    }

    // Mauszeiger bewegen -- MIT der Zeiger-Empfindlichkeit, die von der Kamera-Empfindlichkeit
    // getrennt ist (s. DEFAULT_CURSOR_SENSITIVITY). Eine Stelle fuer beide Zeiger-Faelle
    // (Menue und offenes Overlay), damit sie nicht auseinanderlaufen koennen.
    // Zoom-Modul: den Anteil weich zum Ziel fuehren -- voll, solange Modul an, Taste aktiv und
    // nichts ueber der Welt offen ist (im Inventar oder Chat braucht niemand Zoom).
    pub(crate) fn update_zoom(&mut self) {
        let jetzt = Instant::now();
        let dt = self.zoom_updated.map_or(0.0, |t| jetzt.duration_since(t).as_secs_f32());
        self.zoom_updated = Some(jetzt);
        let aktiv = self.settings.module_zoom && self.zoom_held && !self.screen_overlay_open() && self.chat_input.is_none();
        let ziel = if aktiv { 1.0 } else { 0.0 };
        let k = 1.0 - (-dt / ZOOM_SMOOTH_SECS).exp();
        self.zoom_amount += (ziel - self.zoom_amount) * k;
        if (ziel - self.zoom_amount).abs() < 1e-3 {
            self.zoom_amount = ziel;
        }
    }

    pub(crate) fn move_cursor(&mut self, delta: (f64, f64)) {
        let (w, h) = self.gpu.as_ref().map_or((1920.0, 1080.0), super::super::super::GpuState::size);
        let faktor = self.settings.cursor_sensitivity;
        self.cursor_pos.0 = (self.cursor_pos.0 + delta.0 as f32 * faktor).clamp(0.0, w);
        self.cursor_pos.1 = (self.cursor_pos.1 + delta.1 as f32 * faktor).clamp(0.0, h);
        // Die Drehung haengt an der Zeigerbewegung, nicht an den Maus-Deltas: geklemmt am
        // Bildschirmrand bewegt sich der Zeiger nicht mehr, und dann soll er auch nicht
        // weiterdrehen.
        self.cursor_fx.mouse_move(Instant::now(), self.cursor_pos);
        self.selection_drag();
        self.server_field_drag();
        self.server_bar_drag();
        self.slider_drag_update();
        self.step_slider_drag();
        self.color_popup_drag();
        self.dialog_drag();
        self.drag_scroll_motion();
    }

    // Fester Zeitschritt (physics::TICK_DT = 1/20s) statt "Geschwindigkeit * echte
    // Frame-Zeit" wie frueher bei der reinen Fly-Kamera: Vanillas Formeln (Reibung,
    // Beschleunigung, Sprung) sind PRO TICK definiert, nicht pro Sekunde -- bei 240 FPS
    // wuerden 240 Mini-Ticks pro Sekunde etwas anderes ergeben als Vanillas 20, selbst mit
    // korrekt skalierter Geschwindigkeit (die Reibungs-Rueckkopplung ist nicht linear in
    // der Schrittweite). Der Akkumulator haengt echte Zeit an ganze Ticks; bei starkem
    // Rueckstand (z.B. nach Alt-Tab) wird auf hoechstens ein paar Nachhol-Ticks gedeckelt,
    // damit ein langer Haenger nicht Sekunden an Physik in einem Frame nachrechnet.
    // Ein Paar gegensaetzlicher Richtungen aufloesen. Ohne SnapTap unveraendert
    // durchgereicht (beide gedrueckt = beide aktiv = sie heben sich in der Summe auf, wie
    // Vanilla); mit SnapTap gewinnt die zuletzt gedrueckte Taste, die andere zaehlt so
    // lange als losgelassen.
    pub(crate) fn snap_tap_pair(&self, first: KeyBindAction, second: KeyBindAction) -> (bool, bool) {
        resolve_snap_tap(
            (self.action_active(first), self.keybind_press_order[first.index()]),
            (self.action_active(second), self.keybind_press_order[second.index()]),
            self.settings.snap_tap_enabled,
        )
    }

    // Der Tastenzustand, wie er GILT -- nach SnapTap und nach dem Bildschirm-Gate. Genau
    // diese sieben Flags gehen sowohl in die Physik als auch ins PlayerInput-Paket; getrennt
    // berechnet waren sie ein Anticheat-Flag: Vanillas KeyboardInput.tick() baut den
    // Bewegungsvektor aus DENSELBEN Booleans, die es ins Paket schreibt (im 1.21.11-Jar
    // verifiziert), also muss auch hier gelten "was ich sende, ist was ich laufe".
    //   * SnapTap: das Paket meldete bisher A UND D gedrueckt, waehrend der Client dank
    //     SnapTap tatsaechlich strafte. Fuer Grim ist das Stillstand-laut-Eingabe gegen
    //     Bewegung-laut-Position -- also grim.simulation.
    //   * Offener Bildschirm: MinecraftClient.setScreen ruft KeyBinding.unpressAll(), Vanilla
    //     meldet bei offenem Inventar also durchweg false. Wir standen still, meldeten aber
    //     weiter gedrueckte Tasten -- dieselbe Art Widerspruch.
    //
    // "Offener Bildschirm" heisst dabei JEDER Bildschirm, also auch das Pausemenue und die
    // Einstellungen (AppMode::Menu), nicht nur Inventar/Modul-Menue: seit der Tick auf einem
    // Server auch mit offenem Menue weiterlaeuft (s. update_movement, BadPacketsR), waere ein
    // fehlendes Gate hier ein Spieler, der im Pausemenue weiterlaeuft -- in Vanilla
    // unmoeglich, weil setScreen alle Tasten loest.
    pub(crate) fn movement_input_flags(&self) -> (bool, bool, bool, bool, bool, bool, bool) {
        let (forward, backward) = self.snap_tap_pair(KeyBindAction::Forward, KeyBindAction::Backward);
        let (left, right) = self.snap_tap_pair(KeyBindAction::Left, KeyBindAction::Right);
        let open = self.screen_overlay_open() || self.mode != AppMode::World;
        let gate = |pressed: bool| pressed && !open;
        // Was Addons dazu druecken (s. Call::SetInput) -- auch bei offenem Bildschirm.
        let a = self.addon_input();
        (
            gate(forward) || a.forward,
            gate(backward) || a.back,
            gate(left) || a.left,
            gate(right) || a.right,
            gate(self.jump_held) || a.jump,
            gate(self.sneak_held) || a.sneak,
            gate(self.sprint_held) || a.sprint,
        )
    }
}
