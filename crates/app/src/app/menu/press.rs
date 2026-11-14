// Knoepfe auf Einstellungsseiten wie in osu: beim Druck zieht sich der Knopf ein, ausgeloest
// wird erst beim Loslassen ueber demselben Knopf. Dazu das Aufleuchten eines Abschnitts nach
// einem Klick auf seinen Reiter.

use super::*;
use crate::{settings_row_pos, SETTINGS_CONTENT_DX};

// Beim Druck geht der Knopf in 2 s auf 90 % (anfangs schnell), beim Loslassen federt er in
// 1 s zurueck, kurz ueber 100 % hinaus.
const PRESS_SCALE: f32 = 0.9;
const PRESS_SHRINK_SECS: f32 = 2.0;
const PRESS_SPRING_SECS: f32 = 1.0;
// Aufleuchten des Abschnitts: 0,1 s an, 0,9 s aus.
const SECTION_FLASH_IN: f32 = 0.1;
const SECTION_FLASH_OUT: f32 = 0.9;

fn press_scale_held(t: f32) -> f32 {
    let x = (t / PRESS_SHRINK_SECS).min(1.0);
    1.0 - (1.0 - PRESS_SCALE) * (1.0 - (1.0 - x).powi(5))
}

fn press_scale_released(von: f32, t: f32) -> f32 {
    let x = t / PRESS_SPRING_SECS;
    if x >= 1.0 {
        return 1.0;
    }
    let feder = 2f32.powf(-10.0 * x) * ((10.0 * x - 0.75) * std::f32::consts::TAU / 3.0).sin() + 1.0;
    von + (1.0 - von) * feder
}

fn section_flash_alpha(t: f32) -> f32 {
    if t < SECTION_FLASH_IN {
        1.0 - (1.0 - t / SECTION_FLASH_IN).powi(3)
    } else {
        (1.0 - (t - SECTION_FLASH_IN) / SECTION_FLASH_OUT).max(0.0).powi(2)
    }
}

impl App {
    // Einstellungsknopf unter dem Zeiger. Schieberegler ziehen schon beim Druck, sie zaehlen
    // nicht; der schon gewaehlte Knopf einer Auswahl (Nothing) auch nicht.
    fn settings_press_target(&self) -> Option<ButtonAction> {
        if self.mode != AppMode::Menu || !self.screen.is_settings_category() || self.module_menu_shown() {
            return None;
        }
        let (cx, cy) = self.cursor_pos;
        self.content_buttons()
            .into_iter()
            .find(|b| b.hits(cx, cy))
            .filter(|b| b.slider.is_none() && b.action != ButtonAction::Nothing)
            .map(|b| b.action)
    }

    // Linke Taste auf einem Einstellungsknopf gedrueckt. `aufschieben`: der Klick wartet hier
    // bis zum Loslassen (sonst wartet er schon in drag_scroll). false, wenn dort keiner ist.
    pub(crate) fn settings_press_begin(&mut self, aufschieben: bool) -> bool {
        let Some(action) = self.settings_press_target() else { return false };
        self.settings_press = Some((action, aufschieben));
        self.press_anim = Some((action, Instant::now(), None));
        true
    }

    // Modul-Menue: der Knopf unter dem Zeiger zieht sich beim Druck ein wie auf den
    // Einstellungsseiten; ausgeloest wird dort weiter beim Druck (s. handle_click), das
    // Loslassen laesst ihn nur zurueckfedern.
    pub(crate) fn module_press_begin(&mut self) {
        if !self.module_menu_shown() {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (px, py) = self.pointer();
        let knopf = self.module_menu_buttons(gpu).into_iter().find(|b| self.module_hit(b, px, py)).filter(|b| b.enabled && b.slider.is_none() && b.action != ButtonAction::Nothing);
        if let Some(b) = knopf {
            self.settings_press = Some((b.action, false));
            self.press_anim = Some((b.action, Instant::now(), None));
        }
    }

    // Linke Taste losgelassen: der Knopf federt zurueck; der aufgeschobene Klick zaehlt nur,
    // wenn der Zeiger noch auf demselben Knopf ist.
    pub(crate) fn settings_press_release(&mut self) {
        let Some((action, aufgeschoben)) = self.settings_press.take() else { return };
        self.release_press_anim();
        if aufgeschoben && self.settings_press_target() == Some(action) {
            let (px, py) = self.cursor_pos;
            self.handle_click(px, py);
        }
    }

    // Je Bild: ein Druck, dessen Loslassen nie ankam (Fenster darueber, Seite gewechselt),
    // federt trotzdem zurueck; ausgefedert ist die Animation vorbei.
    pub(crate) fn tick_press_anim(&mut self) {
        if self.settings_press.is_none() {
            self.release_press_anim();
        }
        if let Some((_, _, Some((los, _)))) = self.press_anim {
            if los.elapsed().as_secs_f32() >= PRESS_SPRING_SECS {
                self.press_anim = None;
            }
        }
        if self.section_flash.is_some_and(|(_, _, seit)| seit.elapsed().as_secs_f32() >= SECTION_FLASH_IN + SECTION_FLASH_OUT) {
            self.section_flash = None;
        }
    }

    fn release_press_anim(&mut self) {
        if let Some((action, seit, None)) = self.press_anim {
            self.press_anim = Some((action, seit, Some((Instant::now(), press_scale_held(seit.elapsed().as_secs_f32())))));
        }
    }

    // Zoom fuer push_zoom_marker, solange `action` gedrueckt ist: um die Mitte von `rect`.
    pub(crate) fn press_zoom(&self, action: ButtonAction, rect: Rect) -> Option<(f32, f32, f32)> {
        Some(self.press_scale(action)).filter(|&s| s != 1.0).map(|s| (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, s))
    }

    // Groesse eines Knopfs gerade (1 = normal).
    pub(crate) fn press_scale(&self, action: ButtonAction) -> f32 {
        match self.press_anim {
            Some((a, seit, None)) if a == action => press_scale_held(seit.elapsed().as_secs_f32()),
            Some((a, _, Some((los, von)))) if a == action => press_scale_released(von, los.elapsed().as_secs_f32()),
            _ => 1.0,
        }
    }

    // Gelber Rahmen um alle Zeilen des zuletzt per Reiter angesprungenen Abschnitts
    // (Inhaltskoordinaten, liegt im Zeilenband und scrollt mit).
    pub(crate) fn push_section_flash(&self, screen_w: f32, rects: &mut Vec<UiRect>) {
        let Some((screen, i, seit)) = self.section_flash.filter(|f| f.0 == self.screen) else { return };
        let Some(page) = screen.settings_page() else { return };
        let Some(&(_, erste)) = page.sections.get(i) else { return };
        let letzte = page.sections.get(i + 1).map_or(page.rows, |&(_, row)| row).saturating_sub(1).max(erste);
        let a = section_flash_alpha(seit.elapsed().as_secs_f32());
        if a <= 0.0 {
            return;
        }
        let (starts, stand) = (page.section_starts(), self.page_scroll().stand);
        let y = |row: usize| SETTINGS_FIRST_Y + settings_row_pos(row as f32, starts) - settings_row_pos(stand, starts);
        // Rechts bis kurz hinter die Abschnittslinien (s. build_settings_page_frame).
        let rechts = screen_w - self.side_strip_w(screen_w) - SETTINGS_CONTENT_DX - 16.0;
        let linie_bis = if self.page_max_scroll() > 0 { rechts - 18.0 } else { rechts };
        let (oben, unten) = (y(erste) - 6.0, y(letzte) + SETTINGS_CTRL_H + 6.0);
        let r = Rect { x: SETTINGS_LABEL_X - 12.0, y: oben, w: linie_bis + 6.0 - (SETTINGS_LABEL_X - 12.0), h: unten - oben };
        let gelb = theme::get(theme::Slot::Warning);
        rects.push(UiRect::framed(r, theme::with_alpha(gelb, 0.10 * a), 8.0, 2.0, theme::with_alpha(gelb, a)));
    }
}
