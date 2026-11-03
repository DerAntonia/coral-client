// Feste Rechtecke und Masse der Menue-Bildschirme: Accounts, Serverliste, Pause,
// Einstellungen, Tooltips, Benachrichtigungen.

use crate::*;

// --- Masse der Statusleisten (GUI-Einheiten, s. build_status_bars) --------------------------
// Ein Symbol ist 9x9 und steht im 8er-Raster -- die Symbole ueberlappen sich also um einen
// Pixel, genau wie in Vanilla.
pub(crate) const STATUS_ICON: f32 = 9.0;
pub(crate) const STATUS_ICON_PITCH: f32 = 8.0;
// Abstand zweier Leisten-Zeilen (Ruestung ueber Leben, Luft ueber Nahrung).
pub(crate) const STATUS_ROW_PITCH: f32 = 10.0;
// Breite der Hotbar und damit auch der Erfahrungsleiste; die rechte Kante ist der Nullpunkt
// fuer Nahrung und Luft.
pub(crate) const STATUS_BAR_WIDTH: f32 = 182.0;
// Hoehe des Saettigungsbalkens (Modul "Saettigung") samt Rahmen, in GUI-Pixeln: Vorgabe und
// Modul "Eigenname": laengster Alias, Tempo-Grenzen in Zehntel-Umlaeufen je Sekunde.
pub(crate) const EGO_ALIAS_MAX: usize = 32;
pub(crate) const EGO_SPEED_MIN: u32 = 1;
pub(crate) const EGO_SPEED_MAX: u32 = 30;
// Grenzen des Reglers (s. Settings::saturation_hud_height).
pub(crate) const SATURATION_BAR_HEIGHT_DEFAULT: u32 = 5;
pub(crate) const SATURATION_BAR_HEIGHT_MIN: u32 = 2;
pub(crate) const SATURATION_BAR_HEIGHT_MAX: u32 = 12;
// Wie weit ueber der Hotbar-Oberkante die jeweilige Zeile sitzt (Vanilla-Werte, umgerechnet:
// Herzen height-39, Erfahrungsleiste height-29, Stufenzahl height-35, Hotbar-Oberkante
// height-22).
pub(crate) const STATUS_BARS_ABOVE_HOTBAR: f32 = 17.0;
pub(crate) const XP_BAR_ABOVE_HOTBAR: f32 = 7.0;
pub(crate) const XP_LEVEL_ABOVE_HOTBAR: f32 = 13.0;
pub(crate) const XP_BAR_HEIGHT: f32 = 5.0;
// Itemname ueber der Hotbar (Gui.renderSelectedItemName): Oberkante height-59, also 37 ueber
// der Hotbar; steht zwei Sekunden (40 Ticks bei Vanillas Anzeigedauer 1.0).
pub(crate) const HELD_NAME_ABOVE_HOTBAR: f32 = 37.0;
pub(crate) const HELD_NAME_TICKS: i32 = 40;
// Vanillas Gruen der Stufenzahl (-8323296 = 0x80FF20).
pub(crate) const COLOR_XP_LEVEL: [f32; 4] = [0.5, 1.0, 0.125, 1.0];
// Entity.getMaxAir(): 300 Ticks = 15 Sekunden unter Wasser.
pub(crate) const MAX_AIR_TICKS: i32 = 300;

// Halbtransparenter Hintergrund der Scoreboard-Seitenleiste (wie Vanilla ~40% Schwarz).
pub(crate) const COLOR_SCOREBOARD_BG: [f32; 4] = [0.0, 0.0, 0.0, 0.4];
// --- Tab-Liste (PlayerListHud) ---
// Hintergrund von Kopf, Liste und Fuss: Integer.MIN_VALUE = Schwarz mit halber Deckkraft.
// Deckkraft so, wie Vanilla sie im sRGB-Raum mischt (s. black_veil).
pub(crate) const PLAYER_LIST_BG_ALPHA: f32 = 0.5;
// Jede Zeile: 0x20FFFFFF (Vanillas Vorgabe fuer getTextBackgroundColor), s. white_veil.
pub(crate) const PLAYER_LIST_ROW_ALPHA: f32 = 32.0 / 255.0;
// Zuschauernamen: 0x90FFFFFF.
pub(crate) const PLAYER_LIST_SPECTATOR_ALPHA: f32 = 144.0 / 255.0;
// StyledNumberFormat.YELLOW.
pub(crate) const PLAYER_LIST_SCORE_COLOR: u32 = 0xFFFF55;
pub(crate) const PLAYER_LIST_MAX_ROWS: usize = 20;
pub(crate) const PLAYER_LIST_MAX_ENTRIES: usize = 80;
// Platz fuer die Herzen eines HEARTS-Objectives, in GUI-Einheiten.
pub(crate) const PLAYER_LIST_HEARTS_WIDTH: f32 = 90.0;

// Version, aus deren lokaler Installation Assets (Font, spaeter Texturen) geladen werden.
// Muss zur Protokollversion in protocol::packets passen.
pub(crate) const MC_VERSION: &str = "1.21.11";

// UI-Layout (Menu-Modus), alles in Bildschirm-Pixeln.
pub(crate) const UI_MARGIN: f32 = 40.0;
// Alle Breiten sind an den laengsten Beschriftungen bemessen (in Font-Pixeln mal
// Skalenfaktor, aus der Font-Datei gerechnet) -- ein zu schmaler Button laesst den Text
// herauslaufen statt ihn abzuschneiden.

// Zurueck-Button fuer Unterscreens (Einstellungen/Tastenbelegung/HUD-Editor), oben links --
// ausserhalb des Zeilenrasters, das erst bei y=104 beginnt.
pub(crate) const BACK_BUTTON: Rect = Rect { x: 40.0, y: 30.0, w: 160.0, h: 44.0 };

// Account-Screen: aufgeraeumte vertikale Spalte. Jede Zeile 48px Abstand, ab y=120.
pub(crate) fn account_row_rect(row: usize) -> Rect {
    Rect { x: 40.0, y: 120.0 + row as f32 * 48.0, w: 360.0, h: 40.0 }
}
// Zweite Spalte (z.B. Refresh-Token neben Microsoft) und das Eingabefeld + OK.
pub(crate) fn account_row_rect_col2(row: usize) -> Rect {
    Rect { x: 412.0, y: 120.0 + row as f32 * 48.0, w: 200.0, h: 40.0 }
}
pub(crate) const ACCOUNT_INPUT_FIELD: Rect = Rect { x: 40.0, y: 120.0 + 2.0 * 48.0 + 2.0, w: 460.0, h: 36.0 };
pub(crate) const ACCOUNT_INPUT_OK: Rect = Rect { x: 512.0, y: 120.0 + 2.0 * 48.0 + 2.0, w: 90.0, h: 36.0 };

// Server-Liste rechts auf dem Connect-Screen, wie die Beatmap-Liste in osu!: Karten
// untereinander von der Leiste oben bis fast zum unteren Rand, rechts neben dem Log (endet
// bei x=1000). Anders als in osu! ruecken die Karten nicht seitlich ab. Groesse aus der
// Fenstergroesse (s. UI_SCREEN), damit die Liste jeden Bildschirm ausfuellt.
pub(crate) static UI_SCREEN: [std::sync::atomic::AtomicU32; 2] = [std::sync::atomic::AtomicU32::new(0), std::sync::atomic::AtomicU32::new(0)];

pub(crate) fn ui_screen() -> (f32, f32) {
    use std::sync::atomic::Ordering::Relaxed;
    let (w, h) = (f32::from_bits(UI_SCREEN[0].load(Relaxed)), f32::from_bits(UI_SCREEN[1].load(Relaxed)));
    if w > 0.0 { (w, h) } else { (1920.0, 1080.0) }
}

// Verbindungs-Overlay: so viele letzte Log-Zeilen zeigen die Connect-Phase.
pub(crate) const OVERLAY_LOG_LINES: usize = 6;

// Abbrechen-Knopf im Verbindungs-Overlay, unter den Log-Zeilen (s. build_connecting_overlay).
pub(crate) fn connect_cancel_rect() -> Rect {
    let (sw, sh) = ui_screen();
    let y = (sh * 0.32).round() + 88.0 + OVERLAY_LOG_LINES as f32 * LOG_LINE_HEIGHT + 24.0;
    Rect { x: ((sw - 160.0) / 2.0).round(), y, w: 160.0, h: 40.0 }
}

// Verbindungs-Warnungen (s. App::check_packet_gap, App::note_lagback).
pub(crate) const PACKET_GAP_WARN: Duration = Duration::from_secs(2);
// Die Karte steht, bis wieder etwas kommt -- laenger als jede Stille, die man noch abwartet.
pub(crate) const PACKET_GAP_NOTE_LIFETIME: Duration = Duration::from_mins(2);
// Lag-back: vom Server versetzt, aber nicht weit. Unter 1 cm ist es nur eine Bestaetigung
// der eigenen Stelle, ueber 10 Bloecke ein echter Teleport.
pub(crate) const LAGBACK_MIN_BLOCKS: f64 = 0.01;
pub(crate) const LAGBACK_MAX_BLOCKS: f64 = 10.0;
// Lag-backs mit hoechstens diesem Abstand zaehlen in derselben Karte weiter.
pub(crate) const LAGBACK_GROUP: Duration = Duration::from_secs(2);

// Links daneben stand frueher das Log (40 + 960 breit, jetzt im Launcher); die Liste bleibt,
// wo sie war.
pub(crate) const SERVER_PANEL_X: f32 = 1040.0;

pub(crate) fn server_panel() -> Rect {
    let (sw, sh) = ui_screen();
    let y = TOPBAR_H + 4.0;
    Rect { x: SERVER_PANEL_X, y, w: (sw - SERVER_PANEL_X - 24.0).max(400.0), h: (sh - y - BOTTOM_TILE_H - 12.0).max(300.0) }
}

// Leiste unten wie in osu! (Server-Seite): Kacheln fuer Quick Connect und Neuer Eintrag. Die
// Kacheln sind hoeher als die Leiste und stehen oben ueber ihren Rand hinaus.
pub(crate) const BOTTOMBAR_H: f32 = 44.0;
pub(crate) const BOTTOM_TILE_W: f32 = 200.0;
pub(crate) const BOTTOM_TILE_H: f32 = 64.0;

// Immer drei Kacheln (s. App::bottom_tiles).
pub(crate) const BOTTOM_TILES: usize = 3;

// Kachel `i` der Leiste unten: die erste (Aufnahmen) links unter der Liste der Aufnahmen, die
// uebrigen rechtsbuendig unter der Serverliste, die letzte ganz rechts.
pub(crate) fn bottom_tile_rect(i: usize) -> Rect {
    let (_, sh) = ui_screen();
    if i == 0 {
        return Rect { x: 40.0, y: sh - BOTTOM_TILE_H, w: BOTTOM_TILE_W, h: BOTTOM_TILE_H };
    }
    let p = server_panel();
    let x = p.x + p.w - (BOTTOM_TILES - i) as f32 * (BOTTOM_TILE_W + 12.0) + 12.0;
    Rect { x, y: sh - BOTTOM_TILE_H, w: BOTTOM_TILE_W, h: BOTTOM_TILE_H }
}

// Server-Icon in Bildschirmpixeln: 64 bei Full HD, darueber mitwachsend (QHD skaliert).
pub(crate) fn server_icon_px() -> f32 {
    (server_status::ICON_SIZE as f32 * (ui_screen().1 / 1080.0).max(1.0)).round()
}

// Eine Karte ist EIN Button: ein Klick verbindet, Ziehen verschiebt sie, Rechtsklick oeffnet
// Bearbeiten/Loeschen (s. App::server_drag/server_context).
pub(crate) fn server_row_h() -> f32 {
    server_icon_px() + 24.0
}
pub(crate) const SERVER_ROW_GAP: f32 = 4.0;
// Breite, auf die Vanilla die MOTD umbricht: Zeile der Serverliste (305) ohne Raender, Icon und
// Abstand, in Einheiten der Minecraft-Schrift.
pub(crate) const MOTD_VANILLA_WIDTH: f32 = 267.0;

pub(crate) fn server_list_top() -> f32 {
    server_panel().y
}

pub(crate) fn server_list_bottom() -> f32 {
    let p = server_panel();
    p.y + p.h
}

// So viele Karten passen in die Liste, mit Bruchteil der letzten.
pub(crate) fn server_rows_fit() -> f32 {
    (server_list_bottom() - server_list_top() + SERVER_ROW_GAP) / (server_row_h() + SERVER_ROW_GAP)
}

// So viele Karten passen ganz untereinander; der Rest scrollt.
pub(crate) fn server_rows_visible() -> usize {
    (((server_list_bottom() - server_list_top() + SERVER_ROW_GAP) / (server_row_h() + SERVER_ROW_GAP)).floor() as usize).max(1)
}

// Bildlaufleiste links neben den Karten: Breite, Abstand zur Liste und wie weit daneben ein
// Druck sie noch trifft.
pub(crate) const SERVER_BAR_W: f32 = 8.0;
pub(crate) const SERVER_BAR_GAP: f32 = 10.0;
pub(crate) const SERVER_BAR_HIT: f32 = 6.0;
pub(crate) const SERVER_SEARCH_MAX_CHARS: usize = 64;

// Karte `i` bei Scrollstand `scroll` (in Karten, gleitend).
pub(crate) fn server_card_rect(i: usize, scroll: f32) -> Rect {
    let p = server_panel();
    Rect { x: p.x, y: server_list_top() + (i as f32 - scroll) * (server_row_h() + SERVER_ROW_GAP), w: p.w, h: server_row_h() }
}

// Fenster zum Anlegen/Bearbeiten eines Eintrags in der Bildschirmmitte (wie Quick Connect):
// (Fenster, Adressfeld, Namensfeld, Speichern, Abbrechen).
pub(crate) fn server_dialog_layout() -> (Rect, Rect, Rect, Rect, Rect) {
    let (sw, sh) = ui_screen();
    let (w, h) = (560.0, 446.0);
    let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
    let adresse = Rect { x: p.x + 24.0, y: p.y + 92.0, w: p.w - 48.0, h: 44.0 };
    let name = Rect { y: adresse.y + adresse.h + 40.0, ..adresse };
    let speichern = Rect { x: p.x + p.w - 24.0 - 140.0, y: p.y + p.h - 24.0 - 40.0, w: 140.0, h: 40.0 };
    let abbrechen = Rect { x: speichern.x - 12.0 - 140.0, ..speichern };
    (p, adresse, name, speichern, abbrechen)
}

// Die beiden Knoepfe "Erster Moment" / "Letzter Moment" unter dem Namen.
pub(crate) fn server_dialog_view_buttons() -> [Rect; 2] {
    let (_, _, name, _, _) = server_dialog_layout();
    let w = (name.w - 12.0) / 2.0;
    let erster = Rect { x: name.x, y: name.y + name.h + 40.0, w, h: 40.0 };
    [erster, Rect { x: erster.x + w + 12.0, ..erster }]
}

// Schalter "Namen anzeigen" rechts unter den Verbindungsbild-Knoepfen; die Beschriftung steht
// links daneben.
pub(crate) fn server_dialog_name_toggle() -> Rect {
    let [_, letzter] = server_dialog_view_buttons();
    Rect { x: letzter.x + letzter.w - TOGGLE_W, y: letzter.y + letzter.h + 16.0, w: TOGGLE_W, h: 40.0 }
}

// Rechtsklick-Menue einer Server-Karte: Zeilen "Bearbeiten" und "Loeschen".
pub(crate) const SERVER_CONTEXT_W: f32 = 200.0;
// Token-Export auskommentiert, vorerst abgeschaltet.
// // Konto-Menue: breiter, "Refresh-Token exportieren" passt sonst nicht.
// pub(crate) const ACCOUNT_CONTEXT_W: f32 = 260.0;
pub(crate) const SERVER_CONTEXT_ROW_H: f32 = 40.0;

// Gestapelte Buttons in der Bildschirmmitte: Startbildschirm, Pausemenue (Esc) und
// Gestorben-Screen.
pub(crate) const PAUSE_BTN_W: f32 = 420.0;
pub(crate) const PAUSE_BTN_H: f32 = 52.0;
pub(crate) const PAUSE_FIRST_Y: f32 = 200.0;
pub(crate) const PAUSE_ROW_H: f32 = 68.0;
pub(crate) fn pause_button(row: usize, screen_w: f32) -> Rect {
    Rect { x: ((screen_w - PAUSE_BTN_W) / 2.0).round().max(UI_MARGIN), y: PAUSE_FIRST_Y + row as f32 * PAUSE_ROW_H, w: PAUSE_BTN_W, h: PAUSE_BTN_H }
}

// Wie pause_button, aber so gestapelt, dass Zeile `mitte` genau in der Bildschirmmitte steht.
pub(crate) fn pause_button_around_center(row: usize, mitte: usize, screen_w: f32, screen_h: f32) -> Rect {
    let y = (screen_h - PAUSE_BTN_H) / 2.0 + (row as f32 - mitte as f32) * PAUSE_ROW_H;
    Rect { y: y.round(), ..pause_button(row, screen_w) }
}

// Einstellungen-Tab: Beschriftung links, Bedienelement rechts davon.
// "Maus-Empfindlichkeit" = 202px bei UI_SCALE_SMALL, passt also vor SETTINGS_CTRL_X.
pub(crate) const SETTINGS_FIRST_Y: f32 = 104.0;
pub(crate) const SETTINGS_ROW_H: f32 = 50.0;
pub(crate) const SETTINGS_LABEL_X: f32 = 56.0;
pub(crate) const SETTINGS_CTRL_X: f32 = 380.0;
pub(crate) const SETTINGS_CTRL_H: f32 = 36.0;
pub(crate) const TOGGLE_W: f32 = 120.0;
pub(crate) const STEPPER_BTN_W: f32 = 40.0;
pub(crate) const STEPPER_GAP: f32 = 10.0;
// Breite eines Modulnamens in einer Kachel des Modul-Menues (s. App::module_cols), in
// Bildschirmpixeln wie die Schrift.
pub(crate) const MODULE_TILE_NAME_PX: f32 = 140.0;
// Breit genug fuer "Monitor-Refresh" (166px).
pub(crate) const STEPPER_VALUE_W: f32 = 200.0;

// Anzahl der Einstellungs-Zeilen (0..7, s. SETTINGS_LABELS) -- als Konstante, damit
// Footer/Status-Position sich automatisch mitverschieben, wenn eine Zeile dazukommt. Die
// Kollision mit einer hartkodierten y-Position ist uns hier schon zweimal passiert.
// Die fruehere lange Hinweiszeile darunter ist weg -- Erklaerungen stecken jetzt als
// Hover-Tooltip an der jeweiligen Zeile (s. SETTINGS_TOOLTIPS/hovered_tooltip), nicht mehr
// staendig sichtbar.
pub(crate) const SETTINGS_ROW_COUNT: usize = 17;

// Steuerung-Screen (MenuScreen::SettingsControls): zwei Spalten. Links die Tastenbelegung
// als scrollbare Tabelle (Aktion/Taste/Halten-Umschalten), rechts die uebrigen
// Steuerungs-Einstellungen (Maus-Empfindlichkeit, Verhaltens-Platzhalter) im gewohnten
// Label-links/Bedienelement-rechts-Raster von stepper_value_rect. Feste Pixel-x wie der
// Rest der UI (kein responsives Layout) -- Exclusive-Fullscreen auf der nativen
// Monitor-Aufloesung gibt genug Breite fuer beide Spalten nebeneinander her.
// Tasten-Zeilen der Steuerungs-Seite (ControlsRow::Bind): die Belegung dort, wo sonst Schalter
// und Regler stehen, rechts daneben ein kleines Kaestchen H/T (Halten/Umschalten). Waehrend die
// Zeile auf eine Taste wartet, steht dort "Erweitert": das Fenster mit allen Belegungen der
// Aktion (bis zu MAX_BINDS_PER_ACTION, s. build_keybind_advanced).
pub(crate) const KEYBIND_W: f32 = 200.0;
pub(crate) const KEYBIND_MODE_W: f32 = 40.0;
pub(crate) const KEYBIND_ADVANCED_W: f32 = 100.0;
pub(crate) fn keybind_rect(row: usize) -> Rect {
    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(row), w: KEYBIND_W, h: SETTINGS_CTRL_H }
}
pub(crate) fn keybind_mode_rect(row: usize, wartet: bool) -> Rect {
    let w = if wartet { KEYBIND_ADVANCED_W } else { KEYBIND_MODE_W };
    Rect { x: SETTINGS_CTRL_X + KEYBIND_W + STEPPER_GAP, w, ..keybind_rect(row) }
}

// Schalter und Farbfelder einer Einstellungszeile, an derselben Kante wie die Regler.
pub(crate) fn settings_ctrl_rect(row: usize) -> Rect {
    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(row), w: TOGGLE_W, h: SETTINGS_CTRL_H }
}
// Farbfeld rechts neben dem Schalter seiner Zeile.
pub(crate) fn swatch_after_toggle_rect(row: usize) -> Rect {
    Rect { x: SETTINGS_CTRL_X + TOGGLE_W + STEPPER_GAP, w: SWATCH_W, ..settings_ctrl_rect(row) }
}
// Umschalt-Knopf mit laengerer Beschriftung: so breit wie ein Regler.
pub(crate) fn settings_wide_rect(row: usize) -> Rect {
    Rect { w: STEPPER_VALUE_W + 2.0 * (STEPPER_BTN_W + STEPPER_GAP), ..settings_ctrl_rect(row) }
}

// Einstellungen-Kategorie-Auswahl (MenuScreen::Settings, s. dortigen Kommentar): SETTINGS_
// GENERAL traegt die bisherigen technischen/Performance-Zeilen, die uebrigen je ein Thema.
// Tastenbelegung ist KEINE eigene Kachel mehr (war vorher versehentlich gleichberechtigt
// neben den Kategorien) -- sie haengt jetzt unter Steuerung, wo auch die zugehoerigen
// Verhaltens-Einstellungen (Auto-Sprung, Sprint-/Sneak-Umschaltung, ...) stecken, s.
// MenuScreen::SettingsControls.
pub(crate) const SETTINGS_CATEGORIES: [(MenuScreen, &str); 12] = [
    (MenuScreen::SettingsGeneral, "Render"),
    (MenuScreen::SettingsVideo, "Video"),
    (MenuScreen::SettingsAudio, "Audio"),
    (MenuScreen::SettingsControls, "Steuerung"),
    (MenuScreen::SettingsInventory, "Inventar"),
    (MenuScreen::SettingsSkin, "Skin"),
    (MenuScreen::SettingsFont, "Schrift"),
    (MenuScreen::SettingsDesign, "Aussehen"),
    (MenuScreen::SettingsLanguage, "Sprache"),
    (MenuScreen::SettingsModules, "Module"),
    (MenuScreen::SettingsConfigs, "Configs"),
    (MenuScreen::SettingsMisc, "Sonstiges"),
];
// Zeilen des Sprach-Tabs: Beschriftung und Erklaerung beim Draufzeigen. Zeile 0 stellt alle
// Bereiche auf einmal um, die anderen je einen (Reihenfolge wie in language_of_row).
pub(crate) const LANGUAGE_ROWS: [(&str, &str); 5] = [
    ("Alles", LANGUAGE_HINT),
    ("Menüs und Oberfläche", "Sprache der Menüs, Einstellungen, Hinweise und Meldungen dieses Clients."),
    ("Item-Namen und Tooltips", "Namen von Items und Blöcken, Verzauberungen und Tränke im Tooltip."),
    ("Effekte", "Namen der Effekte im HUD und im Inventar."),
    ("Chat und Server-Texte", "Übersetzte Texte vom Server: Todesnachrichten, Befehlsantworten, Titel, Scoreboard ..."),
];
// Schalter fuer die Laengezeichen, direkt unter den Sprachzeilen.
pub(crate) const LATIN_MACRONS_ROW: usize = LANGUAGE_ROWS.len();
pub(crate) const LATIN_MACRONS_TOOLTIP: &str = "Latein mit Längezeichen (ā ē ī ō ū) – in Menüs und in Item-, Block-, Effekt- und Verzauberungsnamen. AUS zeigt dieselben Wörter ohne Striche, wie Mojangs eigene Lateindatei.";
// Schalter fuer die Lehnwoerter, darunter.
pub(crate) const LOANWORDS_ROW: usize = LATIN_MACRONS_ROW + 1;
pub(crate) const LANGUAGE_HINT: &str = "Welche Sprache wo gilt. \"Alles\" stellt alle Bereiche auf einmal um. Chat und Server-Texte: gilt für neue Nachrichten; dem Server wird die Sprache beim nächsten Verbinden mitgeteilt.";

// Leiste oben (wie in osu!): links das Zahnrad fuer die Einstellungen, rechts Konto,
// Uhrzeit und Benachrichtigungen. Auf allen Menue-Screens ausser HUD-Editor und Tod.
pub(crate) const TOPBAR_H: f32 = 52.0;
pub(crate) const TOPBAR_ICON: f32 = 40.0;
// Kategorienleiste links in den Einstellungen.
pub(crate) const SIDEBAR_W: f32 = 240.0;
pub(crate) const SIDEBAR_ITEM_H: f32 = 36.0;
pub(crate) const SIDEBAR_PAD: f32 = 8.0;
// Links ueber "Setup" in der Kategorienleiste: Name, Adresse, Logo. Die Logos sind weisse
// Masken (getoent wie die anderen Symbole), gerastert aus den Pfaden von Simple Icons.
pub(crate) const SIDEBAR_LINKS: [(&str, &str, &[u8]); 2] = [
    ("GitHub", "https://github.com/DerAntonia/coral-client", include_bytes!("../../../assets/ui/github.png")),
    ("Discord", "https://discord.gg/498TEghpCE", include_bytes!("../../../assets/ui/discord.png")),
];
pub(crate) const LOGO_PX: u32 = 48;
pub(crate) const LOGO_GRID: u32 = 2;
// Groesse des Logos auf dem Schirm, in einem Feld von SIDEBAR_ITEM_H zum Quadrat.
pub(crate) const LOGO_DRAW: f32 = 22.0;
// Um so viel ist der Inhalt einer Einstellungsseite verschoben: seine Rechtecke stehen in
// den alten Koordinaten (Beschriftung bei SETTINGS_LABEL_X), gezeichnet und getroffen wird
// er rechts der Kategorienleiste und unter der Leiste oben (s. App::content_offset).
pub(crate) const SETTINGS_CONTENT_DX: f32 = SIDEBAR_W + 32.0 - SETTINGS_LABEL_X;
pub(crate) const SETTINGS_CONTENT_DY: f32 = 36.0;
// Benachrichtigungen: wie viele die Glocke aufhebt.
pub(crate) const NOTIFICATION_HISTORY: usize = 50;
// Fenster der Glocke (wie in osu!): ein Viertel der Breite rechts unter der Leiste, faehrt
// in dieser Zeit herein bzw. hinaus.
pub(crate) const NOTIFICATION_PANEL_SLIDE: f32 = 0.22;
pub(crate) fn notification_panel_w(screen_w: f32) -> f32 {
    (screen_w / 4.0).max(320.0).min(screen_w).round()
}

pub(crate) const SETTINGS_HUB_BTN_W: f32 = 300.0;
pub(crate) const SETTINGS_HUB_BTN_H: f32 = 52.0;
pub(crate) const SETTINGS_HUB_FIRST_Y: f32 = 140.0;
pub(crate) const SETTINGS_HUB_ROW_H: f32 = 68.0;
pub(crate) const SETTINGS_HUB_COLS: usize = 4;
pub(crate) const SETTINGS_HUB_COL_GAP: f32 = 16.0;
// Kachel `i` von `count`: vier Spalten, als Block mittig, SPALTENWEISE gefuellt -- die
// (alphabetisch sortierte, s. buttons()) Reihenfolge laeuft erst in der ersten Spalte nach
// unten, dann in der naechsten.
pub(crate) fn settings_hub_button(i: usize, count: usize, screen_w: f32) -> Rect {
    let rows = count.div_ceil(SETTINGS_HUB_COLS).max(1);
    let (col, row) = (i / rows, i % rows);
    let block_w = SETTINGS_HUB_COLS as f32 * SETTINGS_HUB_BTN_W + (SETTINGS_HUB_COLS - 1) as f32 * SETTINGS_HUB_COL_GAP;
    let x0 = ((screen_w - block_w) / 2.0).round().max(UI_MARGIN);
    Rect {
        x: x0 + col as f32 * (SETTINGS_HUB_BTN_W + SETTINGS_HUB_COL_GAP),
        y: SETTINGS_HUB_FIRST_Y + row as f32 * SETTINGS_HUB_ROW_H,
        w: SETTINGS_HUB_BTN_W,
        h: SETTINGS_HUB_BTN_H,
    }
}

// Trefferflaeche fuer den Hover-Tooltip einer Einstellungszeile -- deckt Beschriftung UND
// Bedienelement ab (breiter als SETTINGS_CTRL_X + STEPPER_VALUE_W, damit der ganze Stepper
// samt Pfeilen mit reinfaellt), volle Zeilenhoehe statt nur SETTINGS_CTRL_H (grosszuegigere
// Trefferflaeche, kein Zittern beim Rand-Ueberfahren).
pub(crate) fn settings_row_hover_rect(row: usize) -> Rect {
    // Eine angeschnittene Zeile nur mit ihrem sichtbaren Teil (s. settings_row_y_cut).
    let y = settings_row_y(row);
    let oben = if y < 0.0 { y } else { y.max(SETTINGS_CLIP_TOP) };
    Rect { x: SETTINGS_LABEL_X, y: oben, w: 700.0, h: (y + SETTINGS_ROW_H - oben).max(0.0) }
}

// Pixel je Millimeter des Bildschirms (aus der EDID, s. monitor_size), als f32-Bits; 0 = unbekannt.
pub(crate) static PX_PER_MM: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) fn px_per_mm() -> f32 {
    let v = f32::from_bits(PX_PER_MM.load(std::sync::atomic::Ordering::Relaxed));
    // Ohne EDID: 96 dpi.
    if v > 0.0 { v } else { 96.0 / 25.4 }
}

// Rand rechts auf den Einstellungsseiten, in dem die Erklaerung zur Zeile unter dem Zeiger
// steht: 10 cm, auf kleinen Bildschirmen hoechstens ein Viertel der Breite.
pub(crate) fn tooltip_strip_w(screen_w: f32) -> f32 {
    (100.0 * px_per_mm()).min(screen_w * 0.25).round()
}

// Fester Bildschirmbereich rechts fuer den Hover-Tooltip (s. hovered_tooltip) -- bewusst
// NICHT am Mauszeiger, damit die Erklaerung nicht bei jeder Mausbewegung mitspringt und
// nicht ausgerechnet die Zeile verdeckt, die sie erklaert. Hoehe ergibt sich aus dem
// Text (s. Aufrufer), nur x/y/Breite stehen hier fest.
pub(crate) const TOOLTIP_PANEL_W: f32 = 460.0;
pub(crate) const TOOLTIP_PANEL_MARGIN: f32 = 40.0;
// (x, y, Breite) -- die Hoehe haengt vom umgebrochenen Text ab und wird beim Aufrufer
// berechnet, deshalb kein Rect mit einer Blindattrappen-Hoehe hier.
pub(crate) fn tooltip_panel_origin(screen_w: f32) -> (f32, f32, f32) {
    (screen_w - TOOLTIP_PANEL_W - TOOLTIP_PANEL_MARGIN, SETTINGS_FIRST_Y, TOOLTIP_PANEL_W)
}

// Zerlegt einen Tooltip-Text in Zeilen, die je hoechstens `max_width` Pixel breit sind --
// an Wortgrenzen umgebrochen (misst echte Pixelbreite ueber den Font-Atlas statt grob nach
// Zeichenzahl zu schaetzen, s. gpu.measure_text). Fuer Tooltips: kurze, feste Erklaerungen,
// kein Layout-kritischer Pfad, daher ist die pro-Wort-Neumessung hier unproblematisch.
// Mausrad: so viele Pixel eines Touchpads zaehlen als eine Raste.
pub(crate) const WHEEL_PIXELS_PER_NOTCH: f32 = 40.0;
// Einstellungslisten (Tastenbelegungen, Modul-Menue) scrollen eine Zeile je Raste.
pub(crate) const SETTINGS_SCROLL_ROWS_PER_NOTCH: f32 = 1.0;
// Lautstaerke per Mausrad (s. App::wheel_notch): Schritt je Raste in Prozent -- derselbe wie
// die Pfeile im Audio-Tab.
pub(crate) const VOLUME_WHEEL_STEP: u32 = 5;
// Gehaltene Lautstaerke-Taste: erste Wiederholung nach der Pause, danach im Takt (20 %/s).
pub(crate) const VOLUME_KEY_REPEAT_DELAY: Duration = Duration::from_millis(400);
pub(crate) const VOLUME_KEY_REPEAT: Duration = Duration::from_millis(50);
// Anzeige links: Einfahren, Stehen nach der letzten Raste, Ausblenden (Sekunden).
pub(crate) const VOLUME_DISPLAY_ENTER: f32 = 0.25;
pub(crate) const VOLUME_DISPLAY_HOLD: f32 = 1.2;
pub(crate) const VOLUME_DISPLAY_FADE: f32 = 0.3;
pub(crate) const VOLUME_DISPLAY_W: f32 = 200.0;
pub(crate) const VOLUME_DISPLAY_MARGIN: f32 = 16.0;
pub(crate) const VOLUME_BAR_H: f32 = 6.0;

// Aufbau einer Benachrichtigungskarte (s. App::build_notifications): links ein farbiger
// Streifen, daneben Titel und umbrochener Text, unten die Leiste mit der Reststandzeit.
pub(crate) const NOTIFICATION_ACCENT_W: f32 = 4.0;
pub(crate) const NOTIFICATION_PADDING: f32 = 12.0;
pub(crate) const NOTIFICATION_BODY_GAP: f32 = 6.0;
pub(crate) const NOTIFICATION_TIMER_H: f32 = 2.0;

// Streifenfarbe einer Benachrichtigung aus dem Theme (Karte oben rechts und Panel).
pub(crate) fn notification_accent(kind: notifications::Kind) -> [f32; 4] {
    use theme::Slot;
    theme::get(match kind {
        notifications::Kind::Info => Slot::Accent,
        notifications::Kind::Success => Slot::Success,
        notifications::Kind::Warning => Slot::Warning,
        notifications::Kind::Error => Slot::Danger,
    })
}

pub(crate) struct NotificationLayout {
    pub(crate) title: Vec<String>,
    pub(crate) body: Vec<String>,
    pub(crate) height: f32,
}

pub(crate) fn notification_line_h(gpu: &GpuState) -> f32 {
    gpu.text_height(UI_SCALE_SMALL) + 4.0
}

// Eine Quelle fuer Hoehe (Stapeln, Trefferpruefung) und Zeichnen -- sonst laege der
// anklickbare Bereich neben der gezeichneten Karte, sobald sich eins von beiden aendert.
pub(crate) fn notification_layout(gpu: &GpuState, note: &notifications::Notification) -> NotificationLayout {
    let breite = notifications::CARD_WIDTH - NOTIFICATION_ACCENT_W - 2.0 * NOTIFICATION_PADDING;
    // Auch der Titel bricht um: er traegt oft einen Namen (Geraet, Spieler), dessen Laenge
    // niemand vorher kennt.
    let title = wrap_text(gpu, &note.title, UI_SCALE_SMALL, breite);
    let body = if note.body.is_empty() { Vec::new() } else { wrap_text(gpu, &note.body, UI_SCALE_SMALL, breite) };
    let mut height = 2.0 * NOTIFICATION_PADDING + title.len().max(1) as f32 * notification_line_h(gpu) - 4.0;
    if !body.is_empty() {
        height += NOTIFICATION_BODY_GAP + body.len() as f32 * notification_line_h(gpu) - 4.0;
    }
    NotificationLayout { title, body, height }
}
