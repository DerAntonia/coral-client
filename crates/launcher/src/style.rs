// Aussehen des Launchers: die Farben des Standard-Themes aus dem Client (crates/app/src/theme.rs),
// damit Launcher und Client gleich aussehen, und die selbst gezeichneten Teile (Seitenleiste,
// Spielen-Knopf, Karten, Hintergrundbild).

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, Ui, Vec2};

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub const BG_APP: Color32 = rgb(0x0B0D10);
pub const BG_PANEL: Color32 = rgb(0x101317);
pub const SURFACE_SUNKEN: Color32 = rgb(0x0D1013);
pub const SURFACE_1: Color32 = rgb(0x161A1F);
pub const SURFACE_2: Color32 = rgb(0x1C2127);
pub const SURFACE_3: Color32 = rgb(0x242A31);
pub const SURFACE_PRESSED: Color32 = rgb(0x13171B);
pub const BORDER_1: Color32 = rgb(0x2A3038);
pub const BORDER_2: Color32 = rgb(0x3A414B);
pub const BORDER_STRONG: Color32 = rgb(0x4C5561);
pub const TEXT_1: Color32 = rgb(0xE8EBEF);
pub const TEXT_2: Color32 = rgb(0xA4ACB7);
pub const TEXT_ON_ACCENT: Color32 = rgb(0x06111F);
pub const ACCENT: Color32 = rgb(0x4C9EFF);
pub const ACCENT_TEXT: Color32 = rgb(0x9CC8FF);
pub const ACCENT_SURFACE: Color32 = rgb(0x16212D);
pub const ACCENT_SURFACE_HOVER: Color32 = rgb(0x1C2938);
pub const SUCCESS: Color32 = rgb(0x3DD68C);
pub const WARNING: Color32 = rgb(0xF2B53A);
pub const DANGER: Color32 = rgb(0xF0524F);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = BG_APP;
    v.window_fill = BG_PANEL;
    v.extreme_bg_color = SURFACE_SUNKEN;
    v.faint_bg_color = SURFACE_1;
    v.hyperlink_color = ACCENT_TEXT;
    v.warn_fg_color = WARNING;
    v.error_fg_color = DANGER;
    v.window_stroke = Stroke::new(1.0, BORDER_1);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.selection.bg_fill = ACCENT_SURFACE_HOVER;
    v.selection.stroke = Stroke::new(1.0, ACCENT_TEXT);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = SURFACE_1;
    w.noninteractive.weak_bg_fill = SURFACE_1;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_1);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_1);
    for (state, fill, border) in [(&mut w.inactive, SURFACE_2, BORDER_2), (&mut w.hovered, SURFACE_3, BORDER_STRONG), (&mut w.active, SURFACE_PRESSED, BORDER_STRONG), (&mut w.open, SURFACE_3, BORDER_2)] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, border);
        state.fg_stroke = Stroke::new(1.5, TEXT_1);
        state.corner_radius = CornerRadius::same(6);
        state.expansion = 0.0;
    }
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    use egui::TextStyle as T;
    for (text_style, size) in [(T::Heading, 19.0), (T::Body, 14.5), (T::Button, 14.5), (T::Small, 12.0), (T::Monospace, 12.5)] {
        if let Some(font) = style.text_styles.get_mut(&text_style) {
            font.size = size;
        }
    }
    ctx.set_style(style);
}

/// Hintergrund fuer die Seiten neben der Startseite: eine Flaeche je Abschnitt.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = egui::Frame::new()
        .fill(SURFACE_1)
        .stroke(Stroke::new(1.0, BORDER_1))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner;
    ui.add_space(4.0);
    inner
}

/// Eintrag der Seitenleiste. `badge`: Punkt rechts, z.B. fuer ein verfuegbares Update.
pub fn nav_button(ui: &mut Ui, text: &str, selected: bool, badge: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    let fill = if selected {
        ACCENT_SURFACE
    } else if hovered {
        SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, CornerRadius::same(8), fill);
    if selected {
        let strich = Rect::from_center_size(rect.left_center() + egui::vec2(1.5, 0.0), egui::vec2(3.0, 18.0));
        painter.rect_filled(strich, CornerRadius::same(2), ACCENT);
    }
    let color = if selected {
        ACCENT_TEXT
    } else if hovered {
        TEXT_1
    } else {
        TEXT_2
    };
    painter.text(rect.left_center() + egui::vec2(16.0, 0.0), Align2::LEFT_CENTER, text, FontId::proportional(15.0), color);
    if badge {
        painter.circle_filled(rect.right_center() - egui::vec2(16.0, 0.0), 4.0, ACCENT);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Das Bild fuellt `rect` ganz aus (Ueberstand wird mittig abgeschnitten) und ist etwas
/// abgedunkelt, damit Knopf und Titel davor heraustechen. Ohne Bild: eine ruhige Flaeche.
pub fn paint_hero(painter: &egui::Painter, rect: Rect, texture: Option<&egui::TextureHandle>) {
    let Some(texture) = texture else {
        painter.rect_filled(rect, CornerRadius::ZERO, ACCENT_SURFACE);
        return;
    };
    let size = texture.size_vec2();
    let (bild, flaeche) = (size.x / size.y, rect.width() / rect.height());
    let uv = if flaeche > bild {
        let h = bild / flaeche;
        Rect::from_min_max(egui::pos2(0.0, (1.0 - h) / 2.0), egui::pos2(1.0, (1.0 + h) / 2.0))
    } else {
        let w = flaeche / bild;
        Rect::from_min_max(egui::pos2((1.0 - w) / 2.0, 0.0), egui::pos2((1.0 + w) / 2.0, 1.0))
    };
    painter.image(texture.id(), rect, uv, Color32::from_gray(205));
}

/// Der Hinweis oben links im Startbild: Minecrafts Schrift mit Schatten wie im Spiel (ein
/// Schriftpixel nach rechts unten versetzt). Ohne Schrift (keine Installation) die des Launchers.
pub fn paint_hint(painter: &egui::Painter, pos: egui::Pos2, texture: Option<&egui::TextureHandle>, lines: &[String]) {
    const SKALA: f32 = 2.0;
    let Some(texture) = texture else {
        let text = lines.join("\n");
        painter.text(pos + Vec2::splat(1.0), Align2::LEFT_TOP, &text, FontId::proportional(15.0), BG_APP);
        painter.text(pos, Align2::LEFT_TOP, &text, FontId::proportional(15.0), TEXT_1);
        return;
    };
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let rect = Rect::from_min_size(pos, texture.size_vec2() * SKALA);
    painter.image(texture.id(), rect.translate(Vec2::splat(SKALA)), uv, BG_APP);
    painter.image(texture.id(), rect, uv, TEXT_1);
}
