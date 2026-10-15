// Spielmodus-Rad: solange die Taste gehalten wird, steht in der Bildschirmmitte ein Ring aus
// vier Feldern (Ueberleben, Kreativ, Abenteuer, Zuschauer), wie ein Emote-Rad. Die Maus dreht
// dabei nicht den Kopf, sondern schiebt einen Zeiger; beim Loslassen gilt das Feld unter ihm.
// Steht er noch im freien Kreis in der Mitte, gilt der vorige Modus -- so wechselt ein kurzer
// Druck hin und zurueck wie Vanillas F3+F4.
//
// Gewechselt wird wie bei Vanilla mit dem Befehl /gamemode; ohne Rechte antwortet der Server
// mit seiner Fehlermeldung im Chat.

use crate::item_icons::UiSprite;
use crate::{App, AppMode, ConnStatus, GpuState, Rect, UiLine, UiText, GAME_MODE_CREATIVE, GAME_MODE_SURVIVAL};

pub struct Wheel {
    // Zeiger relativ zur Mitte, in Bildschirm-Pixeln.
    offset: (f32, f32),
}

// Im Uhrzeigersinn ab oben: Modus, Beschriftung, Symbol (wie Vanillas Spielmodus-Auswahl),
// Name im Befehl.
const MODES: [(u8, &str, &str, &str); 4] = [
    (0, "Überleben", "iron_sword", "survival"),
    (1, "Kreativ", "grass_block", "creative"),
    (2, "Abenteuer", "map", "adventure"),
    (3, "Zuschauer", "ender_eye", "spectator"),
];

// Ring in GUI-Pixeln: innen frei (die Mitte), aussen das Ende der Felder.
const INNER: f32 = 26.0;
const OUTER: f32 = 78.0;
// Luecke zwischen den Feldern in Grad und Kreisteile je Feld.
const GAP_DEG: f32 = 3.0;
const SLICES: usize = 24;

// Welches Feld unter dem Zeiger liegt; None im freien Kreis.
fn sector(offset: (f32, f32), inner: f32) -> Option<usize> {
    if (offset.0 * offset.0 + offset.1 * offset.1).sqrt() < inner {
        return None;
    }
    let winkel = offset.1.atan2(offset.0).to_degrees();
    Some(((winkel + 135.0).rem_euclid(360.0) / 90.0) as usize % 4)
}

fn label_of(mode: u8) -> &'static str {
    MODES.iter().find(|m| m.0 == mode).map_or("?", |m| m.1)
}

impl App {
    fn gamemode_wheel_scale(&self) -> f32 {
        self.gpu.as_ref().map_or(2.0, |g| self.inventory_scale(g.size()))
    }

    pub(crate) fn set_gamemode_wheel(&mut self, offen: bool) {
        if !offen {
            self.gamemode_wheel = None;
            return;
        }
        let frei = self.mode == AppMode::World && !self.screen_overlay_open() && self.chat_input.is_none() && self.conn_status == ConnStatus::Connected;
        if frei && self.gamemode_wheel.is_none() {
            self.gamemode_wheel = Some(Wheel { offset: (0.0, 0.0) });
        }
    }

    pub(crate) fn gamemode_wheel_mouse(&mut self, delta: (f64, f64)) {
        let max = OUTER * self.gamemode_wheel_scale();
        let Some(w) = self.gamemode_wheel.as_mut() else { return };
        let (x, y) = (w.offset.0 + delta.0 as f32, w.offset.1 + delta.1 as f32);
        let len = (x * x + y * y).sqrt();
        let k = if len > max { max / len } else { 1.0 };
        w.offset = (x * k, y * k);
    }

    // Der Modus, den das Rad beim Loslassen waehlen wuerde.
    fn gamemode_wheel_target(&self) -> Option<u8> {
        let w = self.gamemode_wheel.as_ref()?;
        match sector(w.offset, INNER * self.gamemode_wheel_scale()) {
            Some(i) => Some(MODES[i].0),
            // GameModeSwitcherScreen: ohne vorigen Modus von Kreativ nach Ueberleben, sonst
            // nach Kreativ.
            None => Some(self.previous_game_mode.unwrap_or(if self.game_mode == GAME_MODE_CREATIVE { GAME_MODE_SURVIVAL } else { GAME_MODE_CREATIVE })),
        }
    }

    pub(crate) fn gamemode_wheel_choose(&mut self) {
        let Some(ziel) = self.gamemode_wheel_target() else { return };
        if ziel == self.game_mode {
            return;
        }
        let Some(name) = MODES.iter().find(|m| m.0 == ziel).map(|m| m.3) else { return };
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::chat_command_payload(&format!("gamemode {name}")));
        }
    }

    pub(crate) fn build_gamemode_wheel(&self, gpu: &GpuState, lines: &mut Vec<UiLine>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let Some(w) = self.gamemode_wheel.as_ref() else { return };
        let s = self.gamemode_wheel_scale();
        let (bw, bh) = gpu.size();
        let mitte = (bw / 2.0, bh / 2.0);
        let (innen, aussen) = (INNER * s, OUTER * s);
        let gewaehlt = sector(w.offset, innen);
        let punkt = |r: f32, grad: f32| {
            let a = grad.to_radians();
            (mitte.0 + r * a.cos(), mitte.1 + r * a.sin())
        };
        for (i, &(modus, label, symbol, _)) in MODES.iter().enumerate() {
            // Feld i liegt um -90 + i*90 Grad (oben, rechts, unten, links).
            let von = -135.0 + i as f32 * 90.0 + GAP_DEG / 2.0;
            let bis = von + 90.0 - GAP_DEG;
            let farbe = if gewaehlt == Some(i) {
                [1.0, 1.0, 1.0, 0.45]
            } else if modus == self.game_mode {
                [0.2, 0.2, 0.2, 0.7]
            } else {
                [0.0, 0.0, 0.0, 0.55]
            };
            for k in 0..SLICES {
                let a0 = von + (bis - von) * k as f32 / SLICES as f32;
                let a1 = von + (bis - von) * (k + 1) as f32 / SLICES as f32;
                let quad = [punkt(innen, a0), punkt(aussen, a0), punkt(aussen, a1), punkt(innen, a1)];
                lines.push(UiLine { p0: quad[0], p1: quad[1], color: farbe, width: 0.0, feather: 0.0, quad: Some(quad), quad_colors: None });
            }
            // Symbol und Name in der Feldmitte.
            let (cx, cy) = punkt((innen + aussen) / 2.0, -90.0 + i as f32 * 90.0);
            let groesse = 16.0 * s;
            if let Some(id) = world::item_registry::item_id_by_name(symbol) {
                self.push_item_icon_only(sprites, &protocol::item::ItemStack::new(id, 1), cx - groesse / 2.0, cy - groesse * 0.8, groesse);
            }
            let text_farbe = if modus == self.game_mode { [1.0, 1.0, 0.33, 1.0] } else { [1.0, 1.0, 1.0, 1.0] };
            let feld = Rect { x: cx - 40.0 * s, y: cy + groesse * 0.25, w: 80.0 * s, h: gpu.text_height(s) };
            texts.push(gpu.centered_text(feld, label.to_string(), text_farbe, s));
        }
        // Mitte: wohin es ohne Feld geht.
        if gewaehlt.is_none() {
            if let Some(ziel) = self.gamemode_wheel_target() {
                let h = gpu.text_height(s);
                let feld = Rect { x: mitte.0 - innen, y: mitte.1 - h / 2.0, w: innen * 2.0, h };
                texts.push(gpu.centered_text(feld, label_of(ziel).to_string(), [0.75, 0.75, 0.75, 1.0], s));
            }
        }
        // Den Zeiger zeichnet der normale Mauszeiger (s. gamemode_wheel_cursor).
    }

    // Wo der Mauszeiger steht, solange das Rad offen ist: Bildschirmmitte plus Versatz.
    pub(crate) fn gamemode_wheel_cursor(&self) -> Option<(f32, f32)> {
        let w = self.gamemode_wheel.as_ref()?;
        let (bw, bh) = self.gpu.as_ref()?.size();
        Some((bw / 2.0 + w.offset.0, bh / 2.0 + w.offset.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn felder_im_uhrzeigersinn_ab_oben() {
        assert_eq!(sector((0.0, -50.0), 20.0), Some(0));
        assert_eq!(sector((50.0, 0.0), 20.0), Some(1));
        assert_eq!(sector((0.0, 50.0), 20.0), Some(2));
        assert_eq!(sector((-50.0, 0.0), 20.0), Some(3));
        assert_eq!(sector((5.0, 5.0), 20.0), None);
        // Knapp rechts von oben ist noch oben.
        assert_eq!(sector((30.0, -40.0), 20.0), Some(0));
    }
}
