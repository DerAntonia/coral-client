// Serverliste, Chatverlauf-Datei, Serverdialog und Skin-Spalte.

use crate::*;

// Deckelstand einer Shulkerkiste oder Truhe (ShulkerBoxBlockEntity.animationProgress bzw.
// ChestLidAnimator) -- beide laufen mit 0.1 pro Tick, nur das Zeichnen unterscheidet sich.
#[derive(Clone, Copy, Default)]
pub(crate) struct BlockLid {
    pub(crate) opening: bool,
    pub(crate) progress: f32,
    pub(crate) last: f32,
}

// Ein gespeicherter Server der Liste rechts auf dem Connect-Screen.
#[derive(Clone)]
pub(crate) struct ServerEntry {
    pub(crate) name: String,
    pub(crate) address: String,
    // Verbindungsbild (s. App::connect_view_shown): true = vom letzten Moment (beim Verlassen),
    // false = vom ersten (kurz nach dem Betreten). None = nicht eingestellt, dann nach
    // server_view_last_default.
    pub(crate) view_last: Option<bool>,
    // Namen auf der Karte zeigen. Aus: die Karte zeigt nur die MOTD wie ohne Namen -- solange
    // es eine gibt; sonst (Ping laeuft, offline) steht der Name weiter statt der Adresse.
    pub(crate) show_name: bool,
}

// Ohne eigene Einstellung: Server mit "smp" im Namen oder in der Adresse vom letzten Moment
// (man will sehen, wo man aufgehoert hat), alle anderen vom ersten.
pub(crate) fn server_view_last_default(name: &str, address: &str) -> bool {
    name.to_ascii_lowercase().contains("smp") || address.to_ascii_lowercase().contains("smp")
}

// Die Server-Liste bekommt eine EIGENE Datei statt eines Eintrags in settings.conf: das
// dortige Format ist flach "key=value" pro Zeile und kann keine Liste variabler Laenge mit
// zwei Feldern je Eintrag tragen, ohne sich Indizes in Schluesselnamen zu erfinden.
pub(crate) fn servers_path() -> Option<std::path::PathBuf> {
    shared_config_dir().map(|d| d.join("servers.conf"))
}

// Format: eine Zeile je Server, "Name<TAB>Adresse". Tab als Trenner, weil weder Name noch
// Adresse ihn enthalten koennen (Tabs werden bei der Eingabe als Steuerzeichen verworfen,
// s. push_server_edit_char) -- ein "=" waere in beiden erlaubt und damit mehrdeutig.
pub(crate) fn parse_servers(text: &str) -> Vec<ServerEntry> {
    text.lines()
        .filter_map(|line| {
            let (name, address) = line.split_once('\t')?;
            let address = address.trim();
            if address.is_empty() {
                return None;
            }
            // Dritte Spalte (optional): Verbindungsbild "first" oder "last" ("-" = Vorgabe).
            // Vierte (optional): "noname" = Namen auf der Karte nicht zeigen.
            let mut spalten = address.split('\t');
            let address = spalten.next().unwrap_or("").trim();
            let view_last = match spalten.next().map(str::trim) {
                Some("last") => Some(true),
                Some("first") => Some(false),
                _ => None,
            };
            let show_name = spalten.next().map(str::trim) != Some("noname");
            Some(ServerEntry { name: name.trim().to_string(), address: address.to_string(), view_last, show_name })
        })
        .collect()
}

// Gesendete Chat-Nachrichten (s. chat_input::MAX_HISTORY), eine je Zeile, aelteste zuerst --
// wie die History-Datei einer Shell. Zeilenumbrueche kann die Eingabe nicht enthalten.
pub(crate) fn chat_history_path() -> Option<std::path::PathBuf> {
    shared_config_dir().map(|d| d.join("chat_history.txt"))
}

pub(crate) fn load_chat_history() -> Vec<String> {
    let Some(text) = chat_history_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    let mut verlauf: Vec<String> = text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
    let zuviel = verlauf.len().saturating_sub(chat_input::MAX_HISTORY);
    verlauf.drain(..zuviel);
    verlauf
}

pub(crate) fn save_chat_history(verlauf: &[String]) -> std::io::Result<()> {
    let path = chat_history_path().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "kein Konfigurationsverzeichnis"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body: String = verlauf.iter().map(|z| format!("{}\n", z.replace('\n', " "))).collect();
    std::fs::write(&path, body)
}

// Gespeicherte Konten laden. Gibt es noch keine Liste, kommt das bisher angemeldete Konto
// hinein, damit es mit der Umstellung nicht verschwindet.
pub(crate) fn load_account_store(settings: &Settings) -> accounts::Store {
    let mut store = accounts::load().unwrap_or_default();
    if store.list.is_empty() && !settings.mc_access_token.is_empty() && !settings.account_uuid.is_empty() {
        let creds = auth::Credentials { access_token: settings.mc_access_token.clone(), profile_id: settings.account_uuid.clone(), username: settings.account_name.clone() };
        let msa = (!settings.msa_refresh_token.is_empty()).then(|| (settings.msa_refresh_token.clone(), AZURE_CLIENT_ID.to_string()));
        store.upsert(&creds, msa, accounts::Source::Token);
    }
    store
}

// Wie lange eine Konto-Pruefung gilt, bevor das Oeffnen der Liste sie wiederholt.
pub(crate) const ACCOUNT_RECHECK: std::time::Duration = std::time::Duration::from_mins(1);

pub(crate) fn load_servers() -> Vec<ServerEntry> {
    let Some(path) = servers_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_servers(&text)
}

pub(crate) fn save_servers(servers: &[ServerEntry]) -> std::io::Result<()> {
    let path = servers_path().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "kein Konfigurationsverzeichnis"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body: String = servers
        .iter()
        .map(|s| {
            let ansicht = s.view_last.map(|v| if v { "last" } else { "first" });
            match (ansicht, s.show_name) {
                (Some(v), true) => format!("{}\t{}\t{v}\n", s.name, s.address),
                (None, true) => format!("{}\t{}\n", s.name, s.address),
                (v, false) => format!("{}\t{}\t{}\tnoname\n", s.name, s.address, v.unwrap_or("-")),
            }
        })
        .collect();
    std::fs::write(&path, body)
}

// Welches Feld des Editier-Formulars gerade Tastatureingaben bekommt.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerField {
    Name,
    Address,
}

// Offenes Fenster fuer einen Eintrag der Server-Liste (s. server_dialog_layout). `index` =
// None heisst "neuer Eintrag", der erst beim Speichern angehaengt wird -- so laesst sich das
// Anlegen abbrechen, ohne eine halbfertige Kachel in der Liste zu hinterlassen. Der Name ist
// freiwillig: ohne ihn zeigt die Karte nur die MOTD, in der der Name ohnehin steht.
pub(crate) struct ServerEdit {
    pub(crate) index: Option<usize>,
    pub(crate) name: text_field::TextField,
    pub(crate) address: text_field::TextField,
    pub(crate) focus: ServerField,
    // Linke Taste im fokussierten Feld gedrueckt: Ziehen markiert (s. App::server_field_drag).
    pub(crate) dragging: bool,
    // Letzter Druck ins Feld, fuer den Doppelklick (Wort markieren).
    pub(crate) last_press: Option<(Instant, (f32, f32))>,
    // Verbindungsbild, s. ServerEntry::view_last.
    pub(crate) view_last: Option<bool>,
    // s. ServerEntry::show_name.
    pub(crate) show_name: bool,
}

impl ServerEdit {
    // Was gerade gilt: die eigene Wahl, sonst nach den getippten Feldern.
    pub(crate) fn view_last(&self) -> bool {
        self.view_last.unwrap_or_else(|| server_view_last_default(self.name.text(), self.address.text()))
    }

    pub(crate) const fn field(&self, f: ServerField) -> &text_field::TextField {
        match f {
            ServerField::Name => &self.name,
            ServerField::Address => &self.address,
        }
    }

    pub(crate) const fn focused(&mut self) -> &mut text_field::TextField {
        match self.focus {
            ServerField::Name => &mut self.name,
            ServerField::Address => &mut self.address,
        }
    }
}

pub(crate) const MAX_SERVER_NAME_LEN: usize = 32;

// Offenes Rechtsklick-Menue einer Server-Karte: fuer welchen Eintrag und wo es aufging.
pub(crate) struct ServerContextMenu {
    pub(crate) index: usize,
    pub(crate) at: (f32, f32),
}

// Mit der RECHTEN Taste gedrueckte Server-Karte. Erst ab ein paar Pixeln Bewegung wird daraus ein Ziehen (`moved`,
// bleibt dann gesetzt): die Karte verschwindet aus der Liste, ihr Icon haengt am Zeiger, und
// eine Linie zeigt, wo sie landet. Ohne Bewegung oeffnet das Loslassen das Kontextmenue.
pub(crate) struct ServerDrag {
    pub(crate) index: usize,
    pub(crate) press: (f32, f32),
    pub(crate) moved: std::cell::Cell<bool>,
}
pub(crate) const SERVER_DRAG_START_PX: f32 = 6.0;
// Pause nach einer fehlgeschlagenen Profil-Abfrage eines Kontokopfs (s. account_head_retry).
pub(crate) const ACCOUNT_HEAD_RETRY: Duration = Duration::from_secs(30);
// "Erster Moment": so lange nach dem ersten Chunk wird das Verbindungsbild aufgenommen.
pub(crate) const CONNECT_VIEW_FIRST_DELAY: Duration = Duration::from_secs(1);
// Launcher-Szene (s. App::tick_hero_scene): Start fruehestens so lange nach dem Programmstart,
// mindestens so lange stehen lassen (Upload der letzten Meshes), spaetestens dann aufgeben.
pub(crate) const HERO_START_DELAY: Duration = Duration::from_secs(2);
pub(crate) const HERO_SETTLE: Duration = Duration::from_millis(500);
pub(crate) const HERO_GIVE_UP: Duration = Duration::from_mins(1);
// Die Szene stammt aus einer Overworld: Untergrenze und Zahl der Sections.
pub(crate) const HERO_MIN_Y: i32 = -64;
pub(crate) const HERO_SECTIONS: usize = 24;

// Gezogene Kachel im Skin-Tab -- wie ServerDrag: erst ab SERVER_DRAG_START_PX Bewegung ein
// Ziehen, sonst beim Loslassen ein Klick.
pub(crate) struct SkinDrag {
    pub(crate) column: skin_tab::Column,
    pub(crate) index: usize,
    pub(crate) press: (f32, f32),
    pub(crate) moved: std::cell::Cell<bool>,
}

// Fenster mit dem Dateibaum unter assets/minecraft eines Packs (s. skin_tab::list_tree).
pub(crate) struct SkinFilesWindow {
    // Unter diesem Namen steht das Ausgeschaltete in Settings::skin_exclusions; bei einem
    // Server-Pack wird es zusaetzlich unter `mirror` (Adresse samt UUID) gespeichert.
    pub(crate) skin: String,
    pub(crate) mirror: Option<String>,
    pub(crate) title: String,
    pub(crate) entries: Vec<skin_tab::FileEntry>,
    pub(crate) expanded: std::collections::HashSet<String>,
    // In Zeilen der aufgeklappten Liste.
    pub(crate) scroll: smooth_scroll::SmoothScroll,
}

// Skin-Tab: zwei Spalten mit Kacheln, 4 cm hoch und so breit wie die Spalte.
pub(crate) const SKIN_TILE_GAP: f32 = 10.0;
pub(crate) const SKIN_COLUMN_GAP: f32 = 24.0;
// Ueberschrift ueber den Spalten.
pub(crate) const SKIN_HEAD_H: f32 = 40.0;

pub(crate) fn skin_tile_h() -> f32 {
    (40.0 * px_per_mm()).round().max(80.0)
}

// Spalte in Inhaltskoordinaten (s. App::content_offset): von der Ueberschrift bis unten.
pub(crate) fn skin_column_rect(c: skin_tab::Column) -> Rect {
    let (sw, sh) = ui_screen();
    let rechts = sw - tooltip_strip_w(sw) - SETTINGS_CONTENT_DX - 16.0;
    let w = ((rechts - SETTINGS_LABEL_X - SKIN_COLUMN_GAP) / 2.0).floor();
    let x = match c {
        skin_tab::Column::Available => SETTINGS_LABEL_X,
        skin_tab::Column::Selected => SETTINGS_LABEL_X + w + SKIN_COLUMN_GAP,
    };
    let y = SETTINGS_FIRST_Y - SKIN_HEAD_H;
    Rect { x, y, w, h: settings_rows_bottom(sh) - y }
}

// Wo die Kacheln einer Spalte beginnen und wie viele ganz hineinpassen.
pub(crate) const fn skin_tiles_top() -> f32 {
    SETTINGS_FIRST_Y
}

pub(crate) fn skin_tiles_visible() -> usize {
    let r = skin_column_rect(skin_tab::Column::Available);
    (((r.y + r.h - skin_tiles_top() + SKIN_TILE_GAP) / (skin_tile_h() + SKIN_TILE_GAP)).floor() as usize).max(1)
}

// Kachel auf Platz `platz` (ab der obersten sichtbaren; gleitend, also auch Bruchteile).
pub(crate) fn skin_tile_rect(c: skin_tab::Column, platz: f32) -> Rect {
    let col = skin_column_rect(c);
    Rect { x: col.x, y: skin_tiles_top() + platz * (skin_tile_h() + SKIN_TILE_GAP), w: col.w, h: skin_tile_h() }
}

// Unterkante des Kachelbereichs: was darueber hinausragt, wird abgeschnitten.
pub(crate) fn skin_tiles_bottom() -> f32 {
    let col = skin_column_rect(skin_tab::Column::Available);
    col.y + col.h
}

// Den sichtbaren Teil einer Kachel (zwischen skin_tiles_top und skin_tiles_bottom).
pub(crate) fn skin_clip(r: Rect) -> Option<Rect> {
    let oben = r.y.max(skin_tiles_top());
    let unten = (r.y + r.h).min(skin_tiles_bottom());
    (unten - oben >= 1.0).then_some(Rect { y: oben, h: unten - oben, ..r })
}

// Schwarzer Rand um ein gezogenes Bild (Server-Karte, Pack-Kachel): ein Ring knapp ausserhalb,
// innen durchsichtig. Ausserhalb, weil die Flaechen einer Gruppe vor ihren Sprites gezeichnet
// werden -- ein Rand auf dem Bild laege darunter.
pub(crate) fn drag_icon_rim(feld: Rect) -> UiRect {
    const DICKE: f32 = 2.0;
    let r = Rect { x: feld.x - DICKE, y: feld.y - DICKE, w: feld.w + 2.0 * DICKE, h: feld.h + 2.0 * DICKE };
    UiRect::framed(r, [0.0; 4], 6.0 + DICKE, DICKE, [0.0, 0.0, 0.0, 1.0])
}

// Durchsichtige Pixel eines quadratischen RGBA-Bilds bekommen die Farbe ihrer sichtbaren
// Nachbarn (durchsichtig bleiben sie). Viele Icons haben dort Weiss stehen; wird das Bild
// groesser oder kleiner gezeichnet, mischt die weiche Abtastung diese Farbe in den Rand der
// Form -- ein heller Saum um das Icon (Minemen, Nutzer 2026-11-12).
pub(crate) fn bleed_icon_edges(rgba: &mut [u8], size: u32) {
    let n = size as usize;
    if rgba.len() != n * n * 4 {
        return;
    }
    let alt = rgba.to_vec();
    for y in 0..n {
        for x in 0..n {
            let i = (y * n + x) * 4;
            if alt[i + 3] != 0 {
                continue;
            }
            // Mittel der sichtbaren Nachbarn, nach Deckkraft gewichtet.
            let (mut summe, mut gewicht) = ([0u32; 3], 0u32);
            for ny in y.saturating_sub(1)..=(y + 1).min(n - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(n - 1) {
                    let j = (ny * n + nx) * 4;
                    let a = u32::from(alt[j + 3]);
                    for k in 0..3 {
                        summe[k] += u32::from(alt[j + k]) * a;
                    }
                    gewicht += a;
                }
            }
            for k in 0..3 {
                if let Some(mittel) = summe[k].checked_div(gewicht) {
                    rgba[i + k] = mittel as u8;
                }
            }
        }
    }
}

// Ecken eines quadratischen RGBA-Bilds (`size` Pixel) mit Radius `r` (Bildpixel) abrunden:
// ausserhalb des Viertelkreises durchsichtig, am Rand mit weicher Kante.
pub(crate) fn round_icon_corners(rgba: &mut [u8], size: u32, r: f32) {
    if rgba.len() != (size * size * 4) as usize || r <= 0.0 {
        return;
    }
    let n = size as f32;
    for y in 0..size {
        for x in 0..size {
            // Pixelmitte; nur in den Eckquadraten zaehlt der Abstand zum Kreismittelpunkt.
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = if px < r { r } else if px > n - r { n - r } else { continue };
            let cy = if py < r { r } else if py > n - r { n - r } else { continue };
            let abstand = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let deckung = (r - abstand + 0.5).clamp(0.0, 1.0);
            let a = &mut rgba[((y * size + x) * 4 + 3) as usize];
            *a = (f32::from(*a) * deckung).round() as u8;
        }
    }
}

// Das Pack-Bild links in einer Kachel.
pub(crate) fn skin_icon_rect(tile: Rect) -> Rect {
    let pad = 12.0;
    Rect { x: tile.x + pad, y: tile.y + pad, w: tile.h - 2.0 * pad, h: tile.h - 2.0 * pad }
}

// Fenster mit dem Dateibaum: (Fenster, Liste, "Alles an", "Fertig"). Bildschirmkoordinaten.
pub(crate) fn skin_files_layout() -> (Rect, Rect, Rect, Rect) {
    let (sw, sh) = ui_screen();
    let (w, h) = (720.0_f32.min(sw - 48.0), (sh * 0.8).round());
    let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
    let liste = Rect { x: p.x + 16.0, y: p.y + 84.0, w: p.w - 32.0, h: p.h - 84.0 - 72.0 };
    let fertig = Rect { x: p.x + p.w - 24.0 - 140.0, y: p.y + p.h - 24.0 - 40.0, w: 140.0, h: 40.0 };
    let alles = Rect { x: p.x + 24.0, w: 160.0, ..fertig };
    (p, liste, alles, fertig)
}
pub(crate) const SKIN_FILES_ROW_H: f32 = 30.0;
// Das Dateifenster scrollt drei Zeilen je Raste.
pub(crate) const SKIN_FILES_ROWS_PER_NOTCH: f32 = 3.0;
pub(crate) const SKIN_FILES_INDENT: f32 = 22.0;

pub(crate) fn skin_files_rows_visible() -> usize {
    let (_, liste, _, _) = skin_files_layout();
    ((liste.h / SKIN_FILES_ROW_H).floor() as usize).max(1)
}

pub(crate) const SKIN_TAB_HINT: &str = "Links liegen die Packs aus dem Skin-Ordner, rechts die gewählten: das obere überschreibt die darunter. Packs unter Vanilla füllen nur noch auf, was Vanilla nicht hat. Ein Server-Pack steht anfangs ganz oben und lässt sich wie die anderen verschieben. Rechts verschiebt Ziehen, auch nach links (Esc bricht ab); ein Klick wechselt die Spalte, links schon beim Drücken. Das Zahnrad auf dem Bild schaltet einzelne Dateien und Ordner des Packs aus. Geladen wird, sobald man den Tab verlässt.";
