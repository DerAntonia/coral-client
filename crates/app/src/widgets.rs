// Acc-Manager-Teile, Leiste oben, Icons und Buttons.

use crate::*;

// Zweite Textzeile der Leiste oben ("laeuft seit", Zeitangaben), kleiner als UI_SCALE_SMALL.
pub(crate) const TOPBAR_SUB_SCALE: f32 = 1.5;

// Ein Teil des Acc-Manager-Fensters (s. App::account_manager_layout).
pub(crate) struct AccElem {
    pub(crate) rect: Rect,
    pub(crate) action: Option<ButtonAction>,
    pub(crate) label: String,
    pub(crate) kind: AccKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccountContextItem {
    Logout,
    // Token-Export auskommentiert, vorerst abgeschaltet.
    // ExportToken,
    // ExportRefreshToken,
    Forget,
}

// Token-Export auskommentiert, vorerst abgeschaltet.
// // "Token exportieren" / "Refresh-Token exportieren" (Rechtsklick auf ein Konto): ein Token,
// // das erst nach einer Warnung sichtbar wird. Mit Refresh-Token wird es frisch geholt, sonst
// // ist es das gespeicherte Minecraft-Token. Je Stand der Kontoname dazu; `refresh` = das
// // Refresh-Token statt des Minecraft-Tokens, `client_id` die App, mit der es nur gilt.
// pub(crate) enum TokenExport {
//     // Wird gerade ueber das Refresh-Token erneuert; die Warnung steht schon.
//     Busy { name: String, refresh: bool },
//     // Da, aber noch verdeckt -- erst "Anzeigen" zeigt es.
//     Ready { name: String, refresh: bool, token: String, client_id: Option<String> },
//     Shown { name: String, refresh: bool, token: String, client_id: Option<String> },
//     Failed { name: String, error: String },
// }

// // Knoepfe im Token-Fenster.
// #[derive(Clone, Copy, PartialEq, Eq)]
// pub(crate) enum TokenExportButton {
//     Cancel,
//     Show,
//     Copy,
// }

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccKind {
    Current,
    Other,
    Header,
    Hint,
    Field,
    Button,
    Primary,
    Close,
}

pub(crate) struct TopbarLayout {
    pub(crate) gear: Rect,
    pub(crate) bell: Rect,
    // Trefferflaechen von Zahnrad und Glocke: bis in die Bildschirmecke, damit ein Wurf der
    // Maus in die Ecke trifft (Fitts).
    pub(crate) gear_hit: Rect,
    pub(crate) bell_hit: Rect,
    pub(crate) uhr: Rect,
    pub(crate) kopf: Rect,
    pub(crate) konto: Rect,
    // Linke Kante des Konto-Knopfs ohne die Verbreiterung bei offener Kontenliste: daran
    // richten sich die Tabs, sonst schrumpften sie beim Oeffnen der Liste.
    pub(crate) konto_links: f32,
    pub(crate) name: String,
}

// Zahnrad aus Formen: ein Ring und acht Zaehne rundherum.
pub(crate) fn push_gear_icon(rects: &mut Vec<UiRect>, r: Rect, color: [f32; 4]) {
    push_gear(rects, (r.x + r.w / 2.0, r.y + r.h / 2.0), 8.0, color);
}

// Dasselbe Zahnrad um `mitte`, `r` = Radius des Rings (das Symbol oben hat 8).
pub(crate) fn push_gear(rects: &mut Vec<UiRect>, mitte: (f32, f32), r: f32, color: [f32; 4]) {
    let (cx, cy) = mitte;
    let k = r / 8.0;
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let (zx, zy) = (cx + a.cos() * 8.5 * k, cy + a.sin() * 8.5 * k);
        rects.push(UiRect::rounded(Rect { x: zx - 2.5 * k, y: zy - 2.5 * k, w: 5.0 * k, h: 5.0 * k }, color, 1.2 * k));
    }
    rects.push(UiRect::framed(Rect { x: cx - r, y: cy - r, w: 2.0 * r, h: 2.0 * r }, [0.0; 4], r, 3.5 * k, color));
}

// Gerader Balken von `a` nach `b` (Haken, Pfeile).
pub(crate) fn push_bar(rects: &mut Vec<UiRect>, a: (f32, f32), b: (f32, f32), dicke: f32, color: [f32; 4]) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let lang = (dx * dx + dy * dy).sqrt() + dicke;
    let mitte = (f32::midpoint(a.0, b.0), f32::midpoint(a.1, b.1));
    let r = Rect { x: mitte.0 - lang / 2.0, y: mitte.1 - dicke / 2.0, w: lang, h: dicke };
    rects.push(UiRect { angle: dy.atan2(dx), ..UiRect::rounded(r, color, dicke / 2.0) });
}

// Kaestchen zum Ankreuzen: an = Akzentflaeche mit Haken, aus = leerer Rahmen.
pub(crate) fn push_check_box(rects: &mut Vec<UiRect>, r: Rect, an: bool) {
    use theme::Slot;
    if an {
        rects.push(UiRect::rounded(r, theme::get(Slot::Accent), 4.0));
        let (x, y, s) = (r.x, r.y, r.w / 18.0);
        let farbe = theme::get(Slot::AccentText);
        push_bar(rects, (x + 4.5 * s, y + 9.5 * s), (x + 7.5 * s, y + 12.5 * s), 2.0 * s, farbe);
        push_bar(rects, (x + 7.5 * s, y + 12.5 * s), (x + 13.5 * s, y + 5.5 * s), 2.0 * s, farbe);
    } else {
        rects.push(UiRect::framed(r, theme::get(Slot::SurfaceSunken), 4.0, 1.5, theme::get(Slot::Border2)));
    }
}

// Pfeil zum Auf-/Zuklappen um `mitte`: zu = nach rechts, auf = nach unten.
pub(crate) fn push_chevron(rects: &mut Vec<UiRect>, mitte: (f32, f32), auf: bool, color: [f32; 4]) {
    let (cx, cy) = mitte;
    let (a, b, c) = if auf { ((cx - 4.5, cy - 2.0), (cx, cy + 2.5), (cx + 4.5, cy - 2.0)) } else { ((cx - 2.0, cy - 4.5), (cx + 2.5, cy), (cx - 2.0, cy + 4.5)) };
    push_bar(rects, a, b, 2.0, color);
    push_bar(rects, b, c, 2.0, color);
}

// Pfeil nach links: Schaft und zwei gedrehte Balken als Spitze.
pub(crate) fn push_back_arrow_icon(rects: &mut Vec<UiRect>, r: Rect, color: [f32; 4]) {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let dicke = 3.0;
    rects.push(UiRect::rounded(Rect { x: cx - 9.0, y: cy - dicke / 2.0, w: 18.0, h: dicke }, color, dicke / 2.0));
    let lang = 11.0;
    for winkel in [-std::f32::consts::FRAC_PI_4, std::f32::consts::FRAC_PI_4] {
        // Balken so drehen, dass sein linkes Ende an der Pfeilspitze liegt.
        let (sin, cos) = winkel.sin_cos();
        let mitte = (cx - 9.0 + cos * (lang / 2.0 - dicke / 2.0), cy + sin * (lang / 2.0 - dicke / 2.0));
        rects.push(UiRect { angle: winkel, ..UiRect::rounded(Rect { x: mitte.0 - lang / 2.0, y: mitte.1 - dicke / 2.0, w: lang, h: dicke }, color, dicke / 2.0) });
    }
}

// Glocke: runder Koerper, unten gerade, Rand, Kloeppel.
pub(crate) fn push_bell_icon(rects: &mut Vec<UiRect>, r: Rect, color: [f32; 4]) {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    rects.push(UiRect::rounded(Rect { x: cx - 1.5, y: cy - 11.0, w: 3.0, h: 3.0 }, color, 1.5));
    rects.push(UiRect::rounded(Rect { x: cx - 7.0, y: cy - 9.0, w: 14.0, h: 14.0 }, color, 7.0));
    rects.push(UiRect { rect: Rect { x: cx - 7.0, y: cy - 2.0, w: 14.0, h: 5.0 }, color, ..UiRect::FLAT });
    rects.push(UiRect::rounded(Rect { x: cx - 10.0, y: cy + 3.0, w: 20.0, h: 3.0 }, color, 1.5));
    rects.push(UiRect::rounded(Rect { x: cx - 2.5, y: cy + 7.0, w: 5.0, h: 4.0 }, color, 2.0));
}

// Wie ein Knopf aussieht (Design-System): runde Ecken, Rahmen, Hover eine Stufe heller.
// Die Rolle steckt in button.color: Flaeche 2 = normal, Ausgewaehlt = an/aktiv, Gefahr-Flaeche
// = Konflikt; jede andere Farbe ist eine echte Farbe (Farbfelder) und bleibt, wie sie ist.
// `hover`: wie weit der Knopf gehovert ist (s. App::hover_level) -- 0 = gar nicht, 1 = die
// Hover-Farben des Themes, darueber leuchtet die Flaeche kurz ueber sie hinaus auf.
// Flaeche eines gehoverten Eintrags der Seitenleiste (s. App::hover_level): blendet aus dem
// Leistengrund auf Surface1 ein und leuchtet beim Betreten kurz darueber hinaus zu Surface2.
pub(crate) fn sidebar_hover_fill(hover: f32) -> [f32; 4] {
    use theme::Slot;
    let flaeche = theme::get(Slot::Surface1);
    let grund = mix_color(theme::with_alpha(flaeche, 0.0), flaeche, hover.min(1.0));
    mix_color(grund, theme::get(Slot::Surface2), (hover - 1.0).max(0.0))
}

pub(crate) fn styled_button(button: &Button, hover: f32) -> (UiRect, [f32; 4]) {
    use theme::Slot;
    const RADIUS: f32 = 6.0;
    let g = theme::get;
    // Raender und Text gehen nur bis zur Hover-Farbe, die Flaeche leuchtet darueber hinaus.
    let bis_hover = hover.min(1.0);
    // Reiter: kein Knopf, nur Text; der aktive mit Akzentlinie unten (Design-System, Tabs).
    if matches!(button.action, ButtonAction::SettingsSection(_)) {
        let aktiv = button.color == g(Slot::AccentSurface);
        let linie = Rect { y: button.rect.y + button.rect.h - 2.0, h: 2.0, ..button.rect };
        let farbe = if aktiv { g(Slot::Accent) } else { mix_color(theme::with_alpha(g(Slot::Border2), 0.0), g(Slot::Border2), bis_hover) };
        let text = if aktiv { theme::text() } else { mix_color(theme::text_2(), theme::text(), bis_hover) };
        return (UiRect::rounded(linie, farbe, 1.0), text);
    }
    // Farbfeld: die Farbe selbst, schwarzer Rand, runde Ecken (Design-System).
    if matches!(button.action, ButtonAction::OpenColorPicker(_)) {
        let rand = mix_color([0.0, 0.0, 0.0, 1.0], theme::text_2(), bis_hover);
        return (UiRect::framed(button.rect, button.color, RADIUS, 2.0, rand), theme::text());
    }
    // Unsichtbare Trefferflaeche (Karten, die sich selbst zeichnen).
    if button.color == SERVER_CARD_MARKER && button.label.is_empty() {
        return (UiRect { rect: button.rect, ..UiRect::FLAT }, theme::text());
    }
    if !button.enabled {
        return (UiRect::framed(button.rect, g(Slot::Surface1), RADIUS, 1.0, g(Slot::Border1)), theme::text_disabled());
    }
    let rand = mix_color(g(Slot::Border2), g(Slot::BorderStrong), bis_hover);
    let (fill, border, text) = if button.color == g(Slot::Accent) {
        // Primaer: die eine Hauptaktion der Ansicht.
        let fill = mix_color(g(Slot::Accent), g(Slot::AccentHover), hover);
        (fill, fill, g(Slot::TextOnAccent))
    } else if button.color == g(Slot::AccentSurface) {
        let fill = mix_color(g(Slot::AccentSurface), g(Slot::AccentSurfaceHover), hover);
        (fill, g(Slot::Accent), g(Slot::AccentText))
    } else if button.color == g(Slot::DangerSurface) {
        (button.color, g(Slot::Danger), g(Slot::Danger))
    } else if button.color == g(Slot::Surface2) {
        (mix_color(g(Slot::Surface2), g(Slot::Surface3), hover), rand, theme::text())
    } else {
        (button.color, rand, theme::text())
    };
    (UiRect::framed(button.rect, fill, RADIUS, 1.0, border), text)
}

pub(crate) struct Button {
    // Ein Schieberegler statt eines Knopfs (s. App::step_slider): Klick und Ziehen fuehren
    // dann die Pfeil-Aktionen prev/next aus, bis der Wert unter dem Zeiger liegt.
    pub(crate) slider: Option<StepSlider>,
    pub(crate) action: ButtonAction,
    pub(crate) rect: Rect,
    pub(crate) label: String,
    // Abgeblendete Buttons werden gezeichnet, ignorieren aber Klicks -- so bleibt das
    // Layout stabil und man sieht, dass es die Aktion gibt, sie nur gerade nicht passt.
    pub(crate) enabled: bool,
    pub(crate) color: [f32; 4],
    pub(crate) scale: f32,
}

// Abstand zwischen den Reitern oben auf Einstellungsseiten.
pub(crate) const SECTION_TAB_GAP: f32 = 8.0;
// Wie schnell die Hinterlegung des aktiven Reiters zum neuen Abschnitt gleitet (je Sekunde,
// exponentiell): nach gut 0,15 s ist sie fast da.
pub(crate) const SECTION_SLIDE_RATE: f32 = 18.0;
// So lange gleitet die Leiste oben beim Pausieren von oben herein (Sekunden).
pub(crate) const PAUSE_BAR_SLIDE_SECS: f32 = 0.15;

// Versatz der Leiste oben `secs` nach dem Pausieren: erst ganz ueber dem Bildrand, dann
// abbremsend (kubisch) an ihren Platz.
pub(crate) fn pause_bar_slide_dy(secs: f32) -> f32 {
    let t = (secs / PAUSE_BAR_SLIDE_SECS).clamp(0.0, 1.0);
    -TOPBAR_H * (1.0 - t).powi(3)
}

impl Button {
    // Liegt (x, y) auf dem Knopf? Reiter fangen auch die Haelfte der Luecke zum Nachbarn mit
    // ab -- ein Klick dazwischen soll nicht ins Leere gehen.
    pub(crate) fn hits(&self, x: f32, y: f32) -> bool {
        let r = match self.action {
            ButtonAction::SettingsSection(_) => Rect { x: self.rect.x - SECTION_TAB_GAP / 2.0, w: self.rect.w + SECTION_TAB_GAP, ..self.rect },
            _ => self.rect,
        };
        // Ein angeschnittener Knopf im Zeilenband nur dort, wo er zu sehen ist (s. content_clip).
        // Die Reiter liegen darueber und zaehlen nicht, die Leiste links ragt nur hinein.
        let leiste = r.x + r.w <= SIDEBAR_W;
        if let Some(band) = content_clip().filter(|_| !leiste && !matches!(self.action, ButtonAction::SettingsSection(_))) {
            let schneidet = r.x < band.x + band.w && r.x + r.w > band.x && r.y < band.y + band.h && r.y + r.h > band.y;
            if schneidet && !band.contains(x, y) {
                return false;
            }
        }
        r.contains(x, y)
    }
}

// Der Kern von SnapTap, als reine Funktion (so ohne App-Zustand testbar): je Richtung, ob
// sie aktiv ist und wann sie zuletzt gedrueckt wurde. Ausgeschaltet oder wenn hoechstens
// eine Richtung anliegt, bleibt alles wie es ist -- sonst gewinnt die groessere
// Druck-Nummer, also die zuletzt gedrueckte Taste.
pub(crate) const fn resolve_snap_tap(first: (bool, u64), second: (bool, u64), enabled: bool) -> (bool, bool) {
    let ((a, order_a), (b, order_b)) = (first, second);
    if !(enabled && a && b) {
        return (a, b);
    }
    let newer_is_second = order_b > order_a;
    (!newer_is_second, newer_is_second)
}

// Konto-UUID als 32-stelliger Hex-String (so liefert Mojang sie, ohne Bindestriche) in die
// 16 Rohbytes, die in die Chat-Signatur eingehen.
// UUID als Text mit Bindestrichen (8-4-4-4-12) -- so stehen Nicht-Spieler in Team-Eintraegen.
pub(crate) fn uuid_hyphenated(u: &[u8; 16]) -> String {
    let h: String = u.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub(crate) fn uuid_bytes_from_hex(hex: &str) -> Option<[u8; 16]> {
    let clean: String = hex.chars().filter(|c| *c != '-').collect();
    if clean.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}
