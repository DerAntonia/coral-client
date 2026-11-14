// Benachrichtigungen.

use super::*;

impl App {
    pub(crate) fn notify(&mut self, note: notifications::Notification) -> notifications::NotificationId {
        if self.notification_history.len() >= NOTIFICATION_HISTORY {
            self.notification_history.pop_front();
            self.notifications_seen = self.notifications_seen.saturating_sub(1);
        }
        self.notification_history.push_back((note.title.clone(), note.body.clone(), note.kind, Instant::now()));
        self.notifications.push(note, Instant::now())
    }

    // Wo der Zeiger fuer die Benachrichtigungen steht -- None, solange die Maus die Kamera
    // dreht: dann gibt es keinen Zeiger, und cursor_pos ist ein alter Stand.
    pub(crate) fn notification_pointer(&self) -> Option<(f32, f32)> {
        let frei = match self.mode {
            AppMode::Menu => true,
            AppMode::World => self.screen_overlay_open() || self.chat_input.is_some(),
        };
        (frei && self.focused).then_some(self.cursor_pos)
    }

    // Einmal je Bild, vor dem Zeichnen: Stapel, Animationen und Ziehen weiterschalten.
    pub(crate) fn tick_notifications(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        // Die Karten rechnen immer in Menue-Pixeln, auch im Spiel (s. notification_scale).
        let s = self.notification_scale(gpu);
        let zeiger = gpu.cursor_scale.get() / s;
        let pointer = self.notification_pointer().map(|(x, y)| (x * zeiger, y * zeiger));
        let inset = if self.chrome_visible() { TOPBAR_H } else { 0.0 };
        self.notifications.set_top_inset(inset);
        let vorher = gpu.ui_scale.replace(s);
        self.notifications.update(Instant::now(), gpu.size(), pointer, |n| notification_layout(gpu, n).height);
        gpu.ui_scale.set(vorher);
    }

    // Massstab der Karten: im Menue der des Menues (im HUD-Editor also 1), im Spiel derselbe
    // wie im Menue -- eine Karte ist ueberall gleich gross.
    pub(crate) fn notification_scale(&self, gpu: &crate::GpuState) -> f32 {
        match self.mode {
            AppMode::Menu => gpu.cursor_scale.get(),
            AppMode::World => Self::world_menu_scale(gpu),
        }
    }

    // Die Karten als oberste UI-Ebene (s. UiOverlay). Im Menue rechnet render_menu sie in echte
    // Pixel um, im Spiel geschieht das hier.
    pub(crate) fn build_notifications(&self) -> UiOverlay {
        let mut overlay = UiOverlay::default();
        let Some(gpu) = self.gpu.as_ref() else { return overlay };
        // Zuerst, also unter den Karten.
        self.debug_graph_overlay(&mut overlay);
        let karten_ab = overlay.groups.len();
        let s = self.notification_scale(gpu);
        let vorher = gpu.ui_scale.replace(s);
        self.build_notification_cards(gpu, &mut overlay);
        gpu.ui_scale.set(vorher);
        for g in &mut overlay.groups[karten_ab..] {
            crate::scale_overlay_group(g, s / vorher);
        }
        overlay
    }

    fn build_notification_cards(&self, gpu: &crate::GpuState, overlay: &mut UiOverlay) {
        let now = Instant::now();
        // Fallende Karten zuletzt, also obenauf: sie stuerzen an den stehenden vorbei.
        let mut karten: Vec<_> = self.notifications.cards(now).collect();
        karten.sort_by_key(|k| k.falling);
        for card in karten {
            let a = card.alpha;
            if a <= 0.0 {
                continue;
            }
            let layout = notification_layout(gpu, card.note);
            let mit_alpha = |c: [f32; 4], f: f32| [c[0], c[1], c[2], c[3] * a * f];
            let akzent = notification_accent(card.note.kind);
            let grund = theme::get(if card.hovered { theme::Slot::Surface2 } else { theme::Slot::Surface1 });
            // Gerade stehend auf ganze Pixel: der Bitmap-Font soll scharf bleiben, auch
            // waehrend die Karte gleitet. Gekippt ist das egal -- dann liegt ohnehin kein
            // Texel mehr auf einem Pixel, und Runden liesse sie nur ruckeln.
            let gerade = card.angle == 0.0;
            let px = |v: f32| if gerade { v.round() } else { v };
            let (x, y, w, h) = (px(card.x), px(card.y), card.w, card.h);
            let mut g = UiOverlayGroup { rects: Vec::new(), texts: Vec::new(), sprites: Vec::new(), pivot: card.pivot, angle: card.angle, clip: None };
            // Wie die Karten im Benachrichtigungs-Panel: Flaeche mit Rahmen, Streifen innen.
            let karte = Rect { x, y, w, h };
            g.rects.push(UiRect::shadow(karte, 6.0, 4.0, 16.0, 0.45 * a));
            g.rects.push(UiRect::framed(karte, mit_alpha(grund, 1.0), 6.0, 1.0, mit_alpha(theme::get(theme::Slot::Border1), 1.0)));
            g.rects.push(UiRect::rounded(Rect { x: x + 6.0, y: y + 8.0, w: 3.0, h: h - 16.0 }, mit_alpha(akzent, 1.0), 1.5));
            // Restliche Standzeit als duenne Leiste am unteren Rand.
            let innen = w - 12.0;
            g.rects.push(UiRect::rounded(
                Rect { x: x + 6.0, y: y + h - 4.0 - NOTIFICATION_TIMER_H, w: innen * card.remaining, h: NOTIFICATION_TIMER_H },
                mit_alpha(akzent, 0.5),
                1.0,
            ));
            let tx = x + NOTIFICATION_ACCENT_W + NOTIFICATION_PADDING;
            let mut ty = y + NOTIFICATION_PADDING;
            for line in layout.title {
                g.texts.push(UiText { text: line, x: tx, y: ty, color: mit_alpha(theme::text(), 1.0), scale: UI_SCALE_SMALL });
                ty += notification_line_h(gpu);
            }
            ty += NOTIFICATION_BODY_GAP - 4.0;
            for line in layout.body {
                g.texts.push(UiText { text: line, x: tx, y: ty, color: mit_alpha(theme::text_2(), 1.0), scale: UI_SCALE_SMALL });
                ty += notification_line_h(gpu);
            }
            overlay.groups.push(g);
        }
        self.build_volume_display(overlay);
    }
}
