// Server-Dialoge in der App: Oeffnen, Zeichnen, Klicks, Tasten und die Aktionen der Knoepfe.
// Dazu die beiden Fenster, die zum selben System gehoeren: "Warte auf den Server" (nach
// after_action wait_for_response) und der Verhaltenskodex aus der Konfiguration.
//
// Das Fenster sieht aus wie die uebrigen Fenster des Clients (Abdunkeln, Tafel, Knoepfe im
// Theme); nur die Inhalte -- Texte samt Farben, Items -- kommen vom Server. Breiten gibt der
// Server in GUI-Pixeln an, hier mal GUI_PX.

use std::time::Instant;

use protocol::nbt::TextSpan;

use crate::dialog::{self, ActionButton, AfterAction, Body, Control, Dialog, Event, Kind, Source, Value};
use crate::item_icons::UiSprite;
use crate::text_field::TextField;
use crate::{
    dialog_dim, push_typed_marker, rgb_to_rgba, styled_button, theme, ui_sound, vanilla_text, wrap_spans, App, AppMode, Button, ButtonAction, ChatLine, ConnStatus,
    GpuState, Rect, UiOverlay, UiOverlayGroup, UiRect, UiText, UI_SCALE_NORMAL, UI_SCALE_SMALL,
};

// Bildschirmpixel je GUI-Pixel einer Breitenangabe.
const GUI_PX: f32 = 1.6;
const CONTROL_H: f32 = 36.0;
const PAD: f32 = 24.0;
const GAP: f32 = 12.0;
const BUTTON_GAP: f32 = 6.0;
const CHECK_SIZE: f32 = 20.0;
// So lange bleibt "Zurueck" im Warte-Fenster gesperrt (WaitingForResponseScreen: 5 s, ab
// der ersten Sekunde sichtbar).
const WAIT_BACK_AFTER_S: u64 = 5;

// Ein offener Dialog samt dem Stand seiner Eingaben.
pub(crate) struct OpenDialog {
    pub(crate) dialog: Dialog,
    values: Vec<Value>,
    // Je Eingabe ein Textfeld, nur bei Texteingaben.
    fields: Vec<Option<TextField>>,
    focus: Option<usize>,
    // Die Knoepfe im Inhalt (multi_action, server_links, dialog_list), beim Oeffnen
    // aufgeloest.
    list: Vec<ActionButton>,
    scroll: f32,
    dragging: Option<usize>,
    // Statt des Dialogs der Hinweis auf fremde Fenster (Vanillas Warnknopf neben dem Titel).
    info: bool,
}

impl OpenDialog {
    // Die Werte zum Abschicken: Texteingaben aus ihren Feldern.
    fn current_values(&self) -> Vec<Value> {
        self.values.iter().zip(&self.fields).map(|(v, f)| f.as_ref().map_or_else(|| v.clone(), |f| Value::Text(f.text().to_string()))).collect()
    }
}

pub(crate) struct CodeOfConduct {
    text: String,
    dont_show_again: bool,
    scroll: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ButtonRef {
    Main(usize),
    List(usize),
    Exit,
}

enum Part {
    // Text, mittig je Zeile (PlainMessage).
    Text(Vec<ChatLine>),
    // Beschriftung einer Eingabe, linksbuendig.
    Label(Vec<ChatLine>),
    Item { stack: protocol::item::ItemStack, decorations: bool },
    Field(usize),
    Toggle(usize),
    Cycle(usize),
    Slider(usize),
    Button(ButtonRef),
}

struct Layout {
    panel: Rect,
    title: Vec<ChatLine>,
    title_y: f32,
    info: Rect,
    // Sichtbarer Bereich des Inhalts; was nicht ganz hineinpasst, wird nicht gezeichnet.
    body: Rect,
    max_scroll: f32,
    parts: Vec<(Rect, Part)>,
    footer: Vec<(Rect, ButtonRef)>,
}

fn line_h(gpu: &GpuState) -> f32 {
    gpu.text_height(UI_SCALE_SMALL) + 6.0
}

fn spans_width(gpu: &GpuState, line: &ChatLine) -> f32 {
    line.indent + line.spans.iter().map(|s| gpu.measure_raw(&s.text, UI_SCALE_SMALL)).sum::<f32>()
}

fn wrap(gpu: &GpuState, spans: &[TextSpan], max_w: f32) -> Vec<ChatLine> {
    wrap_spans(spans, max_w.max(40.0), |t| gpu.measure_raw(t, UI_SCALE_SMALL), 0.0)
}

fn span_color(span: &TextSpan) -> [f32; 4] {
    span.color.map(rgb_to_rgba).unwrap_or_else(theme::text)
}

// Eine Zeile farbiger Abschnitte ab `x`.
fn push_line(gpu: &GpuState, texts: &mut Vec<UiText>, line: &ChatLine, x: f32, y: f32) {
    let mut x = x + line.indent;
    for span in &line.spans {
        texts.push(UiText { text: span.text.clone(), x: x.round(), y: y.round(), color: span_color(span), scale: UI_SCALE_SMALL });
        x += gpu.measure_raw(&span.text, UI_SCALE_SMALL);
    }
}

// Abschnitte auf eine Zeile kuerzen, die in `max_w` passt ("…" am Ende).
fn fit_line(gpu: &GpuState, spans: &[TextSpan], max_w: f32) -> ChatLine {
    let mut out: Vec<TextSpan> = Vec::new();
    let mut breite = 0.0;
    let ellipse = gpu.measure_raw("…", UI_SCALE_SMALL);
    for span in spans {
        let text: String = span.text.chars().filter(|c| *c != '\n').collect();
        let w = gpu.measure_raw(&text, UI_SCALE_SMALL);
        if breite + w <= max_w {
            breite += w;
            out.push(TextSpan { text, ..span.clone() });
            continue;
        }
        let mut teil = String::new();
        for c in text.chars() {
            let mut probe = teil.clone();
            probe.push(c);
            if breite + gpu.measure_raw(&probe, UI_SCALE_SMALL) + ellipse > max_w {
                break;
            }
            teil = probe;
        }
        teil.push('…');
        out.push(TextSpan { text: teil, ..span.clone() });
        break;
    }
    ChatLine { indent: 0.0, spans: out }
}

// Links und Klick-Aktionen in einem Textteil: (Flaeche, Ereignis).
fn text_hits(gpu: &GpuState, rect: Rect, lines: &[ChatLine]) -> Vec<(Rect, Event)> {
    let lh = line_h(gpu);
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut x = rect.x + (rect.w - spans_width(gpu, line)) / 2.0 + line.indent;
        let y = rect.y + i as f32 * lh;
        for span in &line.spans {
            let w = gpu.measure_raw(&span.text, UI_SCALE_SMALL);
            let ereignis = span.link.clone().map(Event::OpenUrl).or_else(|| {
                span.action.as_ref().and_then(|a| a.click.as_ref()).map(|k| match k {
                    protocol::nbt::ClickAction::RunCommand(c) => Event::RunCommand(c.clone()),
                    protocol::nbt::ClickAction::SuggestCommand(c) => Event::SuggestCommand(c.clone()),
                    protocol::nbt::ClickAction::CopyToClipboard(c) => Event::CopyToClipboard(c.clone()),
                })
            });
            if let Some(e) = ereignis {
                out.push((Rect { x, y, w, h: lh }, e));
            }
            x += w;
        }
    }
    out
}

// Gespeicherte Annahmen des Verhaltenskodex: je Server-Adresse der Hash des angenommenen
// Textes (wie Vanillas acceptedCodeOfConduct in servers.dat).
fn conduct_path() -> Option<std::path::PathBuf> {
    crate::shared_config_dir().map(|d| d.join("code_of_conduct.conf"))
}

fn load_conduct() -> Vec<(String, i32)> {
    let Some(text) = conduct_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    text.lines().filter_map(|l| l.split_once('\t')).filter_map(|(a, h)| Some((a.to_string(), h.trim().parse().ok()?))).collect()
}

fn save_conduct(liste: &[(String, i32)]) {
    let Some(path) = conduct_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body: String = liste.iter().map(|(a, h)| format!("{a}\t{h}\n")).collect();
    let _ = std::fs::write(path, body);
}

impl App {
    // --- Oeffnen und Schliessen ----------------------------------------------------------

    // Show Dialog vom Server.
    pub(crate) fn show_dialog_ref(&mut self, r: protocol::packets::DialogRef) {
        let daten = match r {
            protocol::packets::DialogRef::Inline(v) => Some(v),
            protocol::packets::DialogRef::Registry(i) => self.dialog_registry.get(i.max(0) as usize).and_then(|(_, d)| d.clone()),
        };
        match daten.as_ref().and_then(dialog::parse) {
            Some(d) => self.open_dialog(d),
            None => self.push_log("Dialog vom Server nicht lesbar -- nicht angezeigt"),
        }
    }

    fn resolve_source(&self, s: &Source) -> Option<Dialog> {
        match s {
            Source::Inline(v) => dialog::parse(v),
            Source::Named(name) => {
                let name = if name.contains(':') { name.clone() } else { format!("minecraft:{name}") };
                self.dialog_registry.iter().find(|(n, _)| *n == name).and_then(|(_, d)| d.as_ref()).and_then(dialog::parse)
            }
        }
    }

    pub(crate) fn open_dialog(&mut self, d: Dialog) {
        let list = match &d.kind {
            Kind::List { buttons, .. } => buttons.clone(),
            Kind::ServerLinks { button_width, .. } => self
                .server_links
                .iter()
                .map(|l| ActionButton {
                    label: match &l.label {
                        protocol::packets::ServerLinkLabel::Known(i) => {
                            let name = protocol::packets::KNOWN_SERVER_LINKS.get(*i as usize).copied().unwrap_or("website");
                            dialog::plain_spans(&vanilla_text(&format!("known_server_link.{name}"), name))
                        }
                        protocol::packets::ServerLinkLabel::Custom(v) => protocol::nbt::text_to_spans(v),
                    },
                    tooltip: None,
                    width: *button_width,
                    action: Some(dialog::Action::OpenUrl(l.url.clone())),
                })
                .collect(),
            Kind::DialogList { dialogs, button_width, .. } => dialogs
                .iter()
                .filter_map(|s| {
                    let unter = self.resolve_source(s)?;
                    Some(ActionButton { label: unter.external_title().to_vec(), tooltip: None, width: *button_width, action: Some(dialog::Action::ShowDialog(s.clone())) })
                })
                .collect(),
            Kind::Notice(_) | Kind::Confirmation { .. } => Vec::new(),
        };
        let values: Vec<Value> = d.inputs.iter().map(|i| i.control.initial_value()).collect();
        let fields: Vec<Option<TextField>> = d
            .inputs
            .iter()
            .map(|i| match &i.control {
                Control::Text { initial, max_length, .. } => Some(TextField::new(initial, *max_length)),
                _ => None,
            })
            .collect();
        let focus = fields.iter().position(Option::is_some);
        let war_offen = self.dialog_shown();
        self.dialog = Some(OpenDialog { dialog: d, values, fields, focus, list, scroll: 0.0, dragging: None, info: false });
        self.dialog_waiting = None;
        if !war_offen && self.mode == AppMode::World {
            self.close_chat();
            if self.inventory.screen_open {
                self.close_inventory();
            }
            if let Some(gpu) = self.gpu.as_ref() {
                self.cursor_pos = (gpu.size().0 / 2.0, gpu.size().1 / 2.0);
            }
            self.clear_controls();
        }
        self.play_ui_sound(ui_sound::UiSound::Popup);
    }

    // Liegt ein Dialog-Fenster (Dialog oder Warten) offen?
    pub(crate) fn dialog_shown(&self) -> bool {
        self.dialog.is_some() || self.dialog_waiting.is_some()
    }

    fn close_dialog_windows(&mut self) {
        let war_offen = self.dialog_shown();
        self.dialog = None;
        self.dialog_waiting = None;
        if war_offen && self.mode == AppMode::World && !self.screen_overlay_open() {
            self.restore_held_controls();
        }
    }

    // Clear Dialog vom Server: der offene Dialog (auch das Warten auf ihn) verschwindet.
    pub(crate) fn clear_dialog(&mut self) {
        self.close_dialog_windows();
    }

    // Alles, was zur Verbindung gehoert, beim Trennen bzw. beim Wechsel in die Welt.
    pub(crate) fn reset_dialogs(&mut self) {
        self.close_dialog_windows();
        self.code_of_conduct = None;
    }

    // --- Aktionen ------------------------------------------------------------------------

    // DialogScreen.runAction: erst entscheiden, was aus dem Fenster wird, dann das Ereignis
    // ausfuehren (ein neuer Dialog ersetzt dabei jedes Fenster).
    fn dialog_run(&mut self, action: Option<dialog::Action>, after: AfterAction) {
        let Some(offen) = self.dialog.as_ref() else { return };
        let ereignis = action.and_then(|a| a.event(&offen.dialog.inputs, &offen.current_values()));
        self.play_ui_sound(ui_sound::UiSound::Click);
        match after {
            AfterAction::None => {}
            AfterAction::Close => self.close_dialog_windows(),
            AfterAction::WaitForResponse => {
                self.dialog = None;
                self.dialog_waiting = Some(Instant::now());
            }
        }
        if let Some(e) = ereignis {
            self.dialog_event(e);
        }
    }

    fn dialog_event(&mut self, e: Event) {
        match e {
            Event::RunCommand(befehl) => {
                let befehl = befehl.strip_prefix('/').unwrap_or(&befehl).to_string();
                // In der Konfiguration gibt es keine Befehle (Vanilla warnt nur).
                if self.conn_status != ConnStatus::Connected {
                    self.push_log(format!("Befehl aus einem Dialog in der Konfiguration verworfen: /{befehl}"));
                    return;
                }
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::chat_command_payload(&befehl));
                }
                self.push_log(format!("Befehl aus einem Dialog: /{befehl}"));
            }
            Event::SuggestCommand(befehl) => {
                if self.mode == AppMode::World && !self.dialog_shown() {
                    self.open_chat();
                    if let Some(feld) = self.chat_input.as_mut() {
                        feld.set_text(&befehl);
                    }
                }
            }
            Event::CopyToClipboard(text) => {
                if let Some(cb) = self.clipboard() {
                    let _ = cb.set_text(text);
                }
            }
            Event::OpenUrl(url) => {
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    self.push_log(format!("Link nicht geöffnet (nur http/https): {url}"));
                    return;
                }
                if let Err(e) = crate::open_external_command(&url).spawn() {
                    self.push_log(format!("Link konnte nicht geöffnet werden: {e}"));
                }
            }
            Event::ShowDialog(s) => match self.resolve_source(&s) {
                Some(d) => self.open_dialog(d),
                None => self.push_log("Verwiesener Dialog unbekannt -- nicht angezeigt"),
            },
            Event::Custom { id, payload } => {
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::custom_click_action_payload(&id, payload.as_ref()));
                }
            }
        }
    }

    fn dialog_button(&self, r: ButtonRef) -> Option<&ActionButton> {
        let offen = self.dialog.as_ref()?;
        match (r, &offen.dialog.kind) {
            (ButtonRef::Main(0), Kind::Notice(b)) => Some(b),
            (ButtonRef::Main(0), Kind::Confirmation { yes, .. }) => Some(yes),
            (ButtonRef::Main(1), Kind::Confirmation { no, .. }) => Some(no),
            (ButtonRef::List(i), _) => offen.list.get(i),
            (ButtonRef::Exit, Kind::List { exit, .. } | Kind::ServerLinks { exit, .. } | Kind::DialogList { exit, .. }) => exit.as_ref(),
            _ => None,
        }
    }

    fn press_dialog_button(&mut self, r: ButtonRef) {
        let Some(b) = self.dialog_button(r) else { return };
        let action = b.action.clone();
        let after = self.dialog.as_ref().map_or(AfterAction::Close, |o| o.dialog.after_action);
        self.dialog_run(action, after);
    }

    // Escape: der Hinweis geht zurueck, das Warten erst nach Ablauf, ein Dialog nur, wenn er
    // es erlaubt -- dann mit seiner Abbruch-Aktion und in jedem Fall geschlossen.
    // true = Escape wurde hier verbraucht.
    pub(crate) fn dialog_escape(&mut self) -> bool {
        if self.code_of_conduct.is_some() {
            return true;
        }
        if let Some(seit) = self.dialog_waiting {
            if seit.elapsed().as_secs() >= WAIT_BACK_AFTER_S {
                self.play_ui_sound(ui_sound::UiSound::Close);
                self.close_dialog_windows();
            }
            return true;
        }
        let Some(offen) = self.dialog.as_mut() else { return false };
        if offen.info {
            offen.info = false;
            return true;
        }
        if offen.dialog.can_close_with_escape {
            let action = offen.dialog.cancel_action().cloned();
            self.dialog_run(action, AfterAction::Close);
        }
        true
    }

    // Das Textfeld, in das gerade getippt wird (s. active_text_field).
    pub(crate) fn dialog_text_field(&mut self) -> Option<&mut TextField> {
        let offen = self.dialog.as_mut().filter(|o| !o.info)?;
        let i = offen.focus?;
        offen.fields.get_mut(i)?.as_mut()
    }

    pub(crate) fn dialog_field_text(&self) -> Option<(usize, String)> {
        let offen = self.dialog.as_ref().filter(|o| !o.info)?;
        let i = offen.focus?;
        Some((i, offen.fields.get(i)?.as_ref()?.text().to_string()))
    }

    // Tab: zum naechsten Textfeld.
    pub(crate) fn dialog_next_field(&mut self) {
        let Some(offen) = self.dialog.as_mut() else { return };
        let felder: Vec<usize> = offen.fields.iter().enumerate().filter(|(_, f)| f.is_some()).map(|(i, _)| i).collect();
        if felder.is_empty() {
            return;
        }
        let jetzt = offen.focus.and_then(|f| felder.iter().position(|&i| i == f));
        offen.focus = Some(felder[jetzt.map_or(0, |p| (p + 1) % felder.len())]);
    }

    // Enter: im mehrzeiligen Feld ein Zeilenumbruch, sonst nichts (wie Vanillas EditBox).
    pub(crate) fn dialog_enter(&mut self) {
        let Some(offen) = self.dialog.as_mut() else { return };
        let Some(i) = offen.focus else { return };
        let mehrzeilig = matches!(offen.dialog.inputs.get(i).map(|x| &x.control), Some(Control::Text { multiline: Some(_), .. }));
        if let (true, Some(Some(feld))) = (mehrzeilig, offen.fields.get_mut(i)) {
            feld.insert("\n");
        }
    }

    pub(crate) fn dialog_scroll(&mut self, delta_px: f32) {
        let max = self.gpu.as_ref().and_then(|g| self.dialog_layout(g)).map_or(0.0, |l| l.max_scroll);
        if let Some(offen) = self.dialog.as_mut() {
            offen.scroll = (offen.scroll - delta_px).clamp(0.0, max);
        }
        if let Some(c) = self.code_of_conduct.as_mut() {
            c.scroll = (c.scroll - delta_px).max(0.0);
        }
    }

    // --- Layout --------------------------------------------------------------------------

    fn dialog_layout(&self, gpu: &GpuState) -> Option<Layout> {
        let offen = self.dialog.as_ref()?;
        let d = &offen.dialog;
        let (sw, sh) = gpu.size();
        let innen_max = (sw - 32.0 - 2.0 * PAD).max(200.0);
        let lh = line_h(gpu);
        let gui = |w: f32| (w * GUI_PX).min(innen_max).round();

        // Erst Groessen sammeln (ohne Lage), dann stapeln.
        let mut stapel: Vec<((f32, f32), Part, f32)> = Vec::new(); // ((Breite, Hoehe), Teil, Abstand davor)
        for b in &d.body {
            match b {
                Body::Text { spans, width } => {
                    let zeilen = wrap(gpu, spans, gui(*width));
                    let w = zeilen.iter().map(|z| spans_width(gpu, z)).fold(0.0, f32::max);
                    stapel.push(((w, zeilen.len() as f32 * lh), Part::Text(zeilen), GAP));
                }
                Body::Item { id, count, description, decorations, width, height } => {
                    let name = id.strip_prefix("minecraft:").unwrap_or(id);
                    let Some(item_id) = world::item_registry::item_id_by_name(name) else { continue };
                    let groesse = (width.max(*height) * 2.0).min(innen_max);
                    stapel.push(((groesse, groesse), Part::Item { stack: protocol::item::ItemStack::new(item_id, *count), decorations: *decorations }, GAP));
                    if let Some((spans, w)) = description {
                        let zeilen = wrap(gpu, spans, gui(*w));
                        let w = zeilen.iter().map(|z| spans_width(gpu, z)).fold(0.0, f32::max);
                        stapel.push(((w, zeilen.len() as f32 * lh), Part::Text(zeilen), 4.0));
                    }
                }
            }
        }
        for (i, input) in d.inputs.iter().enumerate() {
            match &input.control {
                Control::Text { width, label, label_visible, multiline, .. } => {
                    let w = gui(*width);
                    if *label_visible {
                        let zeilen = wrap(gpu, label, w);
                        stapel.push(((w, zeilen.len() as f32 * lh), Part::Label(zeilen), GAP));
                    }
                    let h = match multiline {
                        Some((zeilen, hoehe)) => hoehe.map(|h| h * 2.0).unwrap_or_else(|| (zeilen.unwrap_or(4) as f32 * lh + 16.0).min(512.0)),
                        None => CONTROL_H,
                    };
                    stapel.push(((w, h), Part::Field(i), if *label_visible { 4.0 } else { GAP }));
                }
                Control::Bool { label, .. } => {
                    let w = CHECK_SIZE + 10.0 + label.iter().map(|s| gpu.measure_raw(&s.text, UI_SCALE_SMALL)).sum::<f32>();
                    stapel.push(((w.min(innen_max), CHECK_SIZE.max(lh)), Part::Toggle(i), GAP));
                }
                Control::Choice { width, .. } => stapel.push(((gui(*width), CONTROL_H), Part::Cycle(i), GAP)),
                Control::Range { width, .. } => stapel.push(((gui(*width), CONTROL_H), Part::Slider(i), GAP)),
            }
        }
        // Knoepfe im Inhalt: Spalten, die letzte unvollstaendige Reihe mittig
        // (packControlsIntoColumns).
        let spalten = match &d.kind {
            Kind::List { columns, .. } | Kind::ServerLinks { columns, .. } | Kind::DialogList { columns, .. } => *columns,
            _ => 1,
        };
        let mut reihen: Vec<Vec<(f32, ButtonRef)>> = Vec::new();
        for (i, b) in offen.list.iter().enumerate() {
            if i % spalten == 0 {
                reihen.push(Vec::new());
            }
            if let Some(r) = reihen.last_mut() {
                r.push((gui(b.width), ButtonRef::List(i)));
            }
        }
        let fuss: Vec<(f32, ButtonRef)> = match &d.kind {
            Kind::Notice(b) => vec![(gui(b.width), ButtonRef::Main(0))],
            Kind::Confirmation { yes, no } => vec![(gui(yes.width), ButtonRef::Main(0)), (gui(no.width), ButtonRef::Main(1))],
            Kind::List { exit, .. } | Kind::ServerLinks { exit, .. } | Kind::DialogList { exit, .. } => exit.iter().map(|b| (gui(b.width), ButtonRef::Exit)).collect(),
        };
        let reihe_w = |r: &[(f32, ButtonRef)], gap: f32| r.iter().map(|(w, _)| *w).sum::<f32>() + gap * r.len().saturating_sub(1) as f32;

        let titel_zeilen = wrap(gpu, &d.title, innen_max - 40.0);
        let titel_w = titel_zeilen.iter().map(|z| spans_width(gpu, z)).fold(0.0, f32::max) + 2.0 * 40.0;
        let inhalt_w = stapel
            .iter()
            .map(|((w, _), _, _)| *w)
            .chain(reihen.iter().map(|r| reihe_w(r, BUTTON_GAP)))
            .chain(std::iter::once(reihe_w(&fuss, 8.0)))
            .chain(std::iter::once(titel_w))
            .fold(280.0, f32::max);
        let w = (inhalt_w + 2.0 * PAD).min(sw - 32.0).round();

        let inhalt_h: f32 = stapel.iter().map(|((_, h), _, abstand)| h + abstand).sum::<f32>()
            + reihen.len() as f32 * (CONTROL_H + BUTTON_GAP)
            + if reihen.is_empty() { 0.0 } else { GAP };
        let titel_h = (titel_zeilen.len().max(1) as f32) * (gpu.text_height(UI_SCALE_NORMAL) + 4.0);
        let fuss_h = if fuss.is_empty() { 0.0 } else { CONTROL_H + 20.0 };
        let kopf_h = PAD + titel_h + 8.0;
        let h_voll = kopf_h + inhalt_h + fuss_h + PAD;
        let h = h_voll.min(sh - 32.0).round();
        let panel = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
        let body = Rect { x: panel.x, y: panel.y + kopf_h, w: panel.w, h: (h - kopf_h - fuss_h - PAD).max(0.0) };
        let max_scroll = (inhalt_h - body.h).max(0.0);
        let scroll = offen.scroll.min(max_scroll);

        let mitte = panel.x + panel.w / 2.0;
        let mut parts = Vec::new();
        let mut y = body.y - scroll;
        for ((pw, ph), teil, abstand) in stapel {
            y += abstand;
            let x = if matches!(teil, Part::Label(_)) { mitte - pw.max(0.0) / 2.0 } else { mitte - pw / 2.0 };
            parts.push((Rect { x: x.round(), y: y.round(), w: pw.round(), h: ph.round() }, teil));
            y += ph;
        }
        if !reihen.is_empty() {
            y += GAP;
        }
        for r in reihen {
            let mut x = mitte - reihe_w(&r, BUTTON_GAP) / 2.0;
            for (bw, br) in r {
                parts.push((Rect { x: x.round(), y: y.round(), w: bw, h: CONTROL_H }, Part::Button(br)));
                x += bw + BUTTON_GAP;
            }
            y += CONTROL_H + BUTTON_GAP;
        }
        let mut footer = Vec::new();
        let mut x = mitte - reihe_w(&fuss, 8.0) / 2.0;
        let fy = panel.y + panel.h - PAD - CONTROL_H;
        for (bw, br) in fuss {
            footer.push((Rect { x: x.round(), y: fy, w: bw, h: CONTROL_H }, br));
            x += bw + 8.0;
        }
        let info = Rect { x: panel.x + panel.w - 16.0 - 24.0, y: panel.y + 14.0, w: 24.0, h: 24.0 };
        Some(Layout { panel, title: titel_zeilen, title_y: panel.y + PAD, info, body, max_scroll, parts, footer })
    }

    // Fenster, Text und Knoepfe des Hinweises auf fremde Fenster bzw. des Wartens:
    // (Fenster, Zeilen, [(Knopf, Beschriftung)]).
    fn dialog_notice_layout(&self, gpu: &GpuState) -> Option<(Rect, String, Vec<String>, Vec<Rect>)> {
        let (sw, sh) = gpu.size();
        let w = 560.0_f32.min(sw - 32.0);
        let lh = line_h(gpu);
        let (titel, text, knoepfe) = if let Some(seit) = self.dialog_waiting {
            let s = seit.elapsed().as_secs();
            if s < 1 {
                (vanilla_text("gui.waitingForResponse.title", "Warte auf den Server"), String::new(), 0)
            } else {
                (vanilla_text("gui.waitingForResponse.title", "Warte auf den Server"), String::new(), 1)
            }
        } else if self.dialog.as_ref().is_some_and(|o| o.info) {
            (vanilla_text("menu.custom_screen_info.title", "Hinweis zu eigenen Fenstern"), vanilla_text("menu.custom_screen_info.contents", ""), 2)
        } else {
            return None;
        };
        let zeilen: Vec<String> = text.lines().flat_map(|a| crate::wrap_measured(|t| gpu.measure_raw(t, UI_SCALE_SMALL), a, w - 2.0 * PAD)).collect();
        let titel_h = gpu.text_height(UI_SCALE_NORMAL);
        let h = PAD + titel_h + 20.0 + zeilen.len() as f32 * lh + if knoepfe > 0 { 20.0 + CONTROL_H } else { 0.0 } + PAD;
        let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h: h.min(sh - 32.0) };
        let by = p.y + p.h - PAD - CONTROL_H;
        let rects = match knoepfe {
            1 => vec![Rect { x: (p.x + (p.w - 200.0) / 2.0).round(), y: by, w: 200.0, h: CONTROL_H }],
            2 => vec![Rect { x: (p.x + p.w / 2.0 - 8.0 - 160.0).round(), y: by, w: 160.0, h: CONTROL_H }, Rect { x: (p.x + p.w / 2.0 + 8.0).round(), y: by, w: 160.0, h: CONTROL_H }],
            _ => Vec::new(),
        };
        Some((p, titel, zeilen, rects))
    }

    // --- Zeichnen ------------------------------------------------------------------------

    // Im Menue (Konfiguration, Pausemenue) als eigene Overlay-Gruppe.
    pub(crate) fn build_dialog_overlay(&self, overlay: &mut UiOverlay) {
        if self.mode != AppMode::Menu {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (mut rects, mut texts, mut sprites) = (Vec::new(), Vec::new(), Vec::new());
        self.build_dialog_ui(gpu, &mut rects, &mut texts, &mut sprites);
        if !rects.is_empty() {
            overlay.groups.push(UiOverlayGroup { rects, texts, sprites, pivot: (0.0, 0.0), angle: 0.0 });
        }
    }

    pub(crate) fn build_dialog_ui(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        if self.code_of_conduct.is_some() {
            self.build_code_of_conduct(gpu, rects, texts);
            return;
        }
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (cx, cy) = self.cursor_pos;
        if let Some((p, titel, zeilen, knoepfe)) = self.dialog_notice_layout(gpu) {
            rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT });
            rects.push(UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6));
            rects.push(UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)));
            let titel_r = Rect { x: p.x, y: p.y + PAD, w: p.w, h: gpu.text_height(UI_SCALE_NORMAL) };
            texts.push(gpu.centered_text(titel_r, titel, theme::text(), UI_SCALE_NORMAL));
            let mut y = titel_r.y + titel_r.h + 20.0;
            for z in zeilen {
                texts.push(UiText { text: z, x: p.x + PAD, y, color: theme::text_2(), scale: UI_SCALE_SMALL });
                y += line_h(gpu);
            }
            let beschriftung: Vec<(String, bool, bool)> = if let Some(seit) = self.dialog_waiting {
                let s = seit.elapsed().as_secs();
                if s >= WAIT_BACK_AFTER_S {
                    vec![("Zurück".to_string(), true, false)]
                } else {
                    vec![(format!("Zurück ({}s)", WAIT_BACK_AFTER_S - s), false, false)]
                }
            } else {
                vec![("Trennen".to_string(), true, true), ("Zurück".to_string(), true, false)]
            };
            for (r, (text, aktiv, gefahr)) in knoepfe.into_iter().zip(beschriftung) {
                let b = Button { slider: None, action: ButtonAction::Nothing, rect: r, label: String::new(), enabled: aktiv, color: if gefahr { g(Slot::Danger) } else { g(Slot::Surface2) }, scale: UI_SCALE_SMALL };
                let (flaeche, farbe) = styled_button(&b, self.hover_level(r, aktiv && r.contains(cx, cy)));
                rects.push(flaeche);
                texts.push(gpu.centered_text(r, text, farbe, UI_SCALE_SMALL));
            }
            return;
        }
        let (Some(offen), Some(l)) = (self.dialog.as_ref(), self.dialog_layout(gpu)) else { return };
        let d = &offen.dialog;
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT });
        rects.push(UiRect::shadow(l.panel, 8.0, 12.0, 32.0, 0.6));
        rects.push(UiRect::framed(l.panel, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)));
        // Alles weitere kommt vom Server: nicht uebersetzen.
        push_typed_marker(texts);
        let titel_lh = gpu.text_height(UI_SCALE_NORMAL) + 4.0;
        for (i, z) in l.title.iter().enumerate() {
            let breite: f32 = z.spans.iter().map(|s| gpu.measure_raw(&s.text, UI_SCALE_NORMAL)).sum();
            let mut x = l.panel.x + (l.panel.w - breite) / 2.0;
            for span in &z.spans {
                texts.push(UiText { text: span.text.clone(), x: x.round(), y: l.title_y + i as f32 * titel_lh, color: span_color(span), scale: UI_SCALE_NORMAL });
                x += gpu.measure_raw(&span.text, UI_SCALE_NORMAL);
            }
        }
        // Warnknopf: "i" im Kreis, oeffnet den Hinweis (mit Trennen).
        let info_hover = self.hover_level(l.info, l.info.contains(cx, cy));
        rects.push(UiRect::framed(l.info, if info_hover > 0.0 { g(Slot::Surface2) } else { g(Slot::Surface1) }, 12.0, 1.0, g(Slot::Border2)));
        texts.push(gpu.centered_text(l.info, "i".to_string(), theme::text(), UI_SCALE_SMALL));

        let sichtbar = |r: &Rect| r.y >= l.body.y - 1.0 && r.y + r.h <= l.body.y + l.body.h + 1.0;
        let mut tooltip: Option<Vec<TextSpan>> = None;
        let lh = line_h(gpu);
        for (r, teil) in &l.parts {
            if !sichtbar(r) {
                continue;
            }
            match teil {
                Part::Text(zeilen) => {
                    for (i, z) in zeilen.iter().enumerate() {
                        let x = r.x + (r.w - spans_width(gpu, z)) / 2.0;
                        push_line(gpu, texts, z, x, r.y + i as f32 * lh);
                    }
                    for (hit, _) in text_hits(gpu, *r, zeilen) {
                        rects.push(UiRect { rect: Rect { x: hit.x.round(), y: (hit.y + gpu.text_height(UI_SCALE_SMALL)).round(), w: hit.w.round(), h: 1.0 }, color: theme::text_2(), ..UiRect::FLAT });
                    }
                }
                Part::Label(zeilen) => {
                    for (i, z) in zeilen.iter().enumerate() {
                        push_line(gpu, texts, z, r.x, r.y + i as f32 * lh);
                    }
                }
                Part::Item { stack, decorations } => {
                    self.push_item_icon_only(sprites, stack, r.x, r.y, r.w);
                    if *decorations && stack.count != 1 {
                        let t = stack.count.to_string();
                        let tw = gpu.measure_raw(&t, UI_SCALE_SMALL);
                        texts.push(UiText { text: t, x: (r.x + r.w - tw).round(), y: (r.y + r.h - gpu.text_height(UI_SCALE_SMALL)).round(), color: theme::text(), scale: UI_SCALE_SMALL });
                    }
                }
                Part::Field(i) => {
                    let fokus = offen.focus == Some(*i);
                    rects.push(UiRect::framed(*r, g(Slot::SurfaceSunken), 6.0, 1.0, if fokus { g(Slot::Accent) } else { g(Slot::Border1) }));
                    let Some(Some(feld)) = offen.fields.get(*i) else { continue };
                    if r.h > CONTROL_H {
                        // Mehrzeilig: umbrochen, Schreibmarke am Ende.
                        let zeilen: Vec<String> = feld.text().split('\n').flat_map(|a| crate::wrap_measured(|t| gpu.measure_raw(t, UI_SCALE_SMALL), a, r.w - 24.0)).collect();
                        let mut y = r.y + 8.0;
                        for z in &zeilen {
                            if y + lh > r.y + r.h {
                                break;
                            }
                            texts.push(UiText { text: z.clone(), x: r.x + 12.0, y, color: theme::text(), scale: UI_SCALE_SMALL });
                            y += lh;
                        }
                        if fokus {
                            let letzte = zeilen.last().map_or(0.0, |z| gpu.measure_raw(z, UI_SCALE_SMALL));
                            let cy_ = (r.y + 8.0 + (zeilen.len().max(1) - 1) as f32 * lh).min(r.y + r.h - lh);
                            self.push_caret(rects, Rect { x: r.x + 12.0 + letzte, y: cy_, w: crate::CARET_WIDTH_PX, h: gpu.text_height(UI_SCALE_SMALL) }, theme::text());
                        }
                    } else {
                        self.push_text_field(gpu, feld, *r, fokus, rects, texts);
                    }
                }
                Part::Toggle(i) => {
                    let Some(input) = d.inputs.get(*i) else { continue };
                    let Control::Bool { label, .. } = &input.control else { continue };
                    let an = offen.values.get(*i) == Some(&Value::Bool(true));
                    let kasten = Rect { x: r.x, y: (r.y + (r.h - CHECK_SIZE) / 2.0).round(), w: CHECK_SIZE, h: CHECK_SIZE };
                    let k = self.hover_level(*r, r.contains(cx, cy));
                    rects.push(UiRect::framed(kasten, if an { g(Slot::Accent) } else { g(Slot::SurfaceSunken) }, 4.0, 1.0, if k > 0.0 { g(Slot::Accent) } else { g(Slot::Border2) }));
                    if an {
                        texts.push(gpu.centered_text(kasten, "✓".to_string(), g(Slot::AccentText), UI_SCALE_SMALL));
                    }
                    let z = fit_line(gpu, label, r.w - CHECK_SIZE - 10.0);
                    push_line(gpu, texts, &z, r.x + CHECK_SIZE + 10.0, (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round());
                }
                Part::Cycle(i) => {
                    let Some(input) = d.inputs.get(*i) else { continue };
                    let Control::Choice { label, label_visible, options, .. } = &input.control else { continue };
                    let Some(Value::Choice(wahl)) = offen.values.get(*i) else { continue };
                    let wert = options.get(*wahl).map(|o| o.display.clone()).unwrap_or_default();
                    let mut spans = Vec::new();
                    if *label_visible {
                        spans.extend(label.iter().cloned());
                        spans.push(TextSpan { text: ": ".into(), ..Default::default() });
                    }
                    spans.extend(wert);
                    self.push_dialog_button(gpu, rects, texts, *r, &spans, g(Slot::Surface2), cx, cy);
                }
                Part::Slider(i) => {
                    let Some(input) = d.inputs.get(*i) else { continue };
                    let Control::Range { label, label_format, start, end, initial, step, .. } = &input.control else { continue };
                    let Some(Value::Range(s)) = offen.values.get(*i) else { continue };
                    let k = self.hover_level(*r, r.contains(cx, cy) || offen.dragging == Some(*i));
                    rects.push(UiRect::framed(*r, g(Slot::SurfaceSunken), 6.0, 1.0, if k > 0.0 { g(Slot::Accent) } else { g(Slot::Border1) }));
                    let fill = Rect { x: r.x + 2.0, y: r.y + 2.0, w: ((r.w - 4.0) * s.clamp(0.0, 1.0)).round(), h: r.h - 4.0 };
                    if fill.w > 0.0 {
                        rects.push(UiRect::rounded(fill, g(Slot::AccentSurface), 5.0));
                    }
                    let griff = Rect { x: (r.x + 2.0 + (r.w - 12.0) * s.clamp(0.0, 1.0)).round(), y: r.y + 4.0, w: 8.0, h: r.h - 8.0 };
                    rects.push(UiRect::rounded(griff, g(Slot::Accent), 3.0));
                    let zahl = dialog::number_text(dialog::range_value(*start, *end, *initial, *step, *s));
                    let name: String = label.iter().map(|s| s.text.as_str()).collect();
                    let text = format_label(label_format, &name, &zahl);
                    texts.push(gpu.centered_text(*r, text, theme::text(), UI_SCALE_SMALL));
                }
                Part::Button(br) => {
                    let Some(b) = self.dialog_button(*br) else { continue };
                    if r.contains(cx, cy) {
                        tooltip = b.tooltip.clone();
                    }
                    self.push_dialog_button(gpu, rects, texts, *r, &b.label, g(Slot::Surface2), cx, cy);
                }
            }
        }
        // Bildlaufleiste, wenn der Inhalt nicht passt.
        if l.max_scroll > 0.0 {
            let inhalt = l.body.h + l.max_scroll;
            let h = (l.body.h * l.body.h / inhalt).max(24.0);
            let y = l.body.y + (l.body.h - h) * (offen.scroll.min(l.max_scroll) / l.max_scroll);
            rects.push(UiRect::rounded(Rect { x: l.panel.x + l.panel.w - 10.0, y, w: 4.0, h }, g(Slot::Border2), 2.0));
        }
        for (i, (r, br)) in l.footer.iter().enumerate() {
            let Some(b) = self.dialog_button(*br) else { continue };
            if r.contains(cx, cy) {
                tooltip = b.tooltip.clone();
            }
            // Der erste Knopf unten ist der Hauptknopf (OK, Ja).
            let farbe = if i == 0 && matches!(br, ButtonRef::Main(0)) { g(Slot::Accent) } else { g(Slot::Surface2) };
            self.push_dialog_button(gpu, rects, texts, *r, &b.label, farbe, cx, cy);
        }
        if let Some(t) = tooltip {
            let (te, fe) = (texts.len(), rects.len());
            self.build_chat_tooltip(gpu, &t, self.chat_scale(), self.chat_line_height(gpu), rects, texts, te, fe);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_dialog_button(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, r: Rect, label: &[TextSpan], farbe: [f32; 4], cx: f32, cy: f32) {
        let b = Button { slider: None, action: ButtonAction::Nothing, rect: r, label: String::new(), enabled: true, color: farbe, scale: UI_SCALE_SMALL };
        let (flaeche, textfarbe) = styled_button(&b, self.hover_level(r, r.contains(cx, cy)));
        rects.push(flaeche);
        let z = fit_line(gpu, label, r.w - 16.0);
        let breite = spans_width(gpu, &z);
        let y = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let mut x = r.x + (r.w - breite) / 2.0;
        for span in &z.spans {
            // Ohne eigene Farbe in der Farbe des Knopfes.
            let c = span.color.map(rgb_to_rgba).unwrap_or(textfarbe);
            texts.push(UiText { text: span.text.clone(), x: x.round(), y, color: c, scale: UI_SCALE_SMALL });
            x += gpu.measure_raw(&span.text, UI_SCALE_SMALL);
        }
    }

    // --- Maus ----------------------------------------------------------------------------

    // Linke (oder rechte) Taste bei offenem Dialogfenster. Schluckt jeden Druck.
    pub(crate) fn dialog_press(&mut self, rechts: bool) {
        if self.code_of_conduct.is_some() {
            if !rechts {
                self.code_of_conduct_press();
            }
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (cx, cy) = self.cursor_pos;
        if let Some((_, _, _, knoepfe)) = self.dialog_notice_layout(gpu) {
            if rechts {
                return;
            }
            if let Some(seit) = self.dialog_waiting {
                if knoepfe.first().is_some_and(|r| r.contains(cx, cy)) && seit.elapsed().as_secs() >= WAIT_BACK_AFTER_S {
                    self.play_ui_sound(ui_sound::UiSound::Close);
                    self.close_dialog_windows();
                }
                return;
            }
            if knoepfe.first().is_some_and(|r| r.contains(cx, cy)) {
                // Wie Vanillas "Custom screen rejected".
                self.play_ui_sound(ui_sound::UiSound::Close);
                self.disconnect();
                self.disconnect_popup = Some(vanilla_text("menu.custom_screen_info.disconnect", "Fenster des Servers abgelehnt"));
            } else if knoepfe.get(1).is_some_and(|r| r.contains(cx, cy)) {
                self.play_ui_sound(ui_sound::UiSound::Close);
                if let Some(o) = self.dialog.as_mut() {
                    o.info = false;
                }
            }
            return;
        }
        let Some(l) = self.dialog_layout(gpu) else { return };
        if !rechts && l.info.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Popup);
            if let Some(o) = self.dialog.as_mut() {
                o.info = true;
            }
            return;
        }
        if let Some((_, br)) = l.footer.iter().find(|(r, _)| r.contains(cx, cy)).filter(|_| !rechts) {
            self.press_dialog_button(*br);
            return;
        }
        if !l.body.contains(cx, cy) {
            return;
        }
        let treffer = l.parts.iter().find(|(r, _)| r.contains(cx, cy) && r.y >= l.body.y - 1.0 && r.y + r.h <= l.body.y + l.body.h + 1.0);
        let Some((r, teil)) = treffer else { return };
        let r = *r;
        match teil {
            Part::Button(br) if !rechts => self.press_dialog_button(*br),
            Part::Text(zeilen) if !rechts => {
                let klick = text_hits(gpu, r, zeilen).into_iter().find(|(h, _)| h.contains(cx, cy)).map(|(_, e)| e);
                if let Some(e) = klick {
                    // Wie runActionOnParent: das Ereignis mit der after_action des Dialogs.
                    let after = self.dialog.as_ref().map_or(AfterAction::Close, |o| o.dialog.after_action);
                    self.play_ui_sound(ui_sound::UiSound::Click);
                    match after {
                        AfterAction::None => {}
                        AfterAction::Close => self.close_dialog_windows(),
                        AfterAction::WaitForResponse => {
                            self.dialog = None;
                            self.dialog_waiting = Some(Instant::now());
                        }
                    }
                    self.dialog_event(e);
                }
            }
            Part::Field(i) if !rechts => {
                let i = *i;
                let mehrzeilig = r.h > CONTROL_H;
                let x0 = r.x + 12.0;
                if let Some(offen) = self.dialog.as_mut() {
                    offen.focus = Some(i);
                    if let (false, Some(Some(feld))) = (mehrzeilig, offen.fields.get_mut(i)) {
                        let idx = crate::text_field::index_at(feld.text(), cx - x0, |t| gpu.measure_raw(t, UI_SCALE_NORMAL));
                        feld.press(idx, false);
                    }
                }
            }
            Part::Toggle(i) => {
                let i = *i;
                if let Some(Value::Bool(b)) = self.dialog.as_mut().and_then(|o| o.values.get_mut(i)) {
                    *b = !*b;
                }
                self.play_ui_sound(ui_sound::UiSound::Click);
            }
            Part::Cycle(i) => {
                let i = *i;
                if let Some(offen) = self.dialog.as_mut() {
                    let anzahl = match offen.dialog.inputs.get(i).map(|x| &x.control) {
                        Some(Control::Choice { options, .. }) => options.len(),
                        _ => 0,
                    };
                    if let (Some(Value::Choice(w)), true) = (offen.values.get_mut(i), anzahl > 0) {
                        *w = if rechts { (*w + anzahl - 1) % anzahl } else { (*w + 1) % anzahl };
                    }
                }
                self.play_ui_sound(ui_sound::UiSound::Click);
            }
            Part::Slider(i) if !rechts => {
                let i = *i;
                if let Some(offen) = self.dialog.as_mut() {
                    offen.dragging = Some(i);
                }
                self.dialog_drag();
            }
            _ => {}
        }
    }

    // Ziehen am Regler (Maus bewegt bei gedrueckter Taste).
    pub(crate) fn dialog_drag(&mut self) {
        let Some(i) = self.dialog.as_ref().and_then(|o| o.dragging) else { return };
        let Some(r) = self.gpu.as_ref().and_then(|g| self.dialog_layout(g)).and_then(|l| l.parts.into_iter().find(|(_, p)| matches!(p, Part::Slider(j) if *j == i)).map(|(r, _)| r)) else {
            return;
        };
        let s = ((self.cursor_pos.0 - r.x - 6.0) / (r.w - 12.0)).clamp(0.0, 1.0);
        if let Some(Value::Range(v)) = self.dialog.as_mut().and_then(|o| o.values.get_mut(i)) {
            if (*v - s).abs() > f32::EPSILON {
                *v = s;
                self.play_ui_sound(ui_sound::UiSound::Slider);
            }
        }
    }

    pub(crate) fn dialog_release(&mut self) {
        if let Some(o) = self.dialog.as_mut() {
            o.dragging = None;
        }
    }

    // --- Verhaltenskodex -----------------------------------------------------------------

    // Code of Conduct aus der Konfiguration: schon fuer diesen Text angenommen -> gleich
    // bestaetigen, sonst fragen.
    pub(crate) fn code_of_conduct_received(&mut self, text: String) {
        let hash = dialog::java_hash(&text);
        if load_conduct().iter().any(|(a, h)| *a == self.server_address && *h == hash) {
            if let Some(tx) = &self.outgoing_tx {
                let _ = tx.send(protocol::packets::accept_code_of_conduct_payload());
            }
            return;
        }
        self.code_of_conduct = Some(CodeOfConduct { text, dont_show_again: false, scroll: 0.0 });
        self.play_ui_sound(ui_sound::UiSound::Popup);
    }

    // (Fenster, Zeilen, Haekchen-Zeile, Trennen, Annehmen, sichtbare Zeilen)
    fn code_of_conduct_layout(&self, gpu: &GpuState) -> Option<(Rect, Vec<String>, Rect, Rect, Rect, usize)> {
        let c = self.code_of_conduct.as_ref()?;
        let (sw, sh) = gpu.size();
        let w = 640.0_f32.min(sw - 32.0);
        let lh = line_h(gpu);
        let zeilen: Vec<String> = c.text.lines().flat_map(|a| crate::wrap_measured(|t| gpu.measure_raw(t, UI_SCALE_SMALL), a, w - 2.0 * PAD)).collect();
        let titel_h = gpu.text_height(UI_SCALE_NORMAL);
        let rest = PAD + titel_h + 20.0 + 16.0 + CHECK_SIZE + 20.0 + CONTROL_H + PAD;
        let platz = ((sh - 32.0 - rest) / lh).floor().max(3.0) as usize;
        let sichtbar = zeilen.len().min(platz);
        let h = rest + sichtbar as f32 * lh;
        let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
        let haken = Rect { x: p.x + PAD, y: p.y + p.h - PAD - CONTROL_H - 20.0 - CHECK_SIZE, w: p.w - 2.0 * PAD, h: CHECK_SIZE };
        let by = p.y + p.h - PAD - CONTROL_H;
        let annehmen = Rect { x: p.x + p.w - PAD - 160.0, y: by, w: 160.0, h: CONTROL_H };
        let trennen = Rect { x: annehmen.x - 12.0 - 160.0, ..annehmen };
        Some((p, zeilen, haken, trennen, annehmen, sichtbar))
    }

    fn build_code_of_conduct(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let (Some(c), Some((p, zeilen, haken, trennen, annehmen, sichtbar))) = (self.code_of_conduct.as_ref(), self.code_of_conduct_layout(gpu)) else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (cx, cy) = self.cursor_pos;
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT });
        rects.push(UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6));
        rects.push(UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)));
        let titel = Rect { x: p.x, y: p.y + PAD, w: p.w, h: gpu.text_height(UI_SCALE_NORMAL) };
        texts.push(gpu.centered_text(titel, vanilla_text("multiplayer.codeOfConduct.title", "Verhaltenskodex des Servers"), theme::text(), UI_SCALE_NORMAL));
        let lh = line_h(gpu);
        let max_start = zeilen.len().saturating_sub(sichtbar);
        let start = ((c.scroll / lh).floor() as usize).min(max_start);
        push_typed_marker(texts);
        let mut y = titel.y + titel.h + 20.0;
        for z in zeilen.iter().skip(start).take(sichtbar) {
            texts.push(UiText { text: z.clone(), x: p.x + PAD, y, color: theme::text_2(), scale: UI_SCALE_SMALL });
            y += lh;
        }
        let kasten = Rect { x: haken.x, y: haken.y, w: CHECK_SIZE, h: CHECK_SIZE };
        let k = self.hover_level(haken, haken.contains(cx, cy));
        rects.push(UiRect::framed(kasten, if c.dont_show_again { g(Slot::Accent) } else { g(Slot::SurfaceSunken) }, 4.0, 1.0, if k > 0.0 { g(Slot::Accent) } else { g(Slot::Border2) }));
        if c.dont_show_again {
            texts.push(gpu.centered_text(kasten, "✓".to_string(), g(Slot::AccentText), UI_SCALE_SMALL));
        }
        let ty = (haken.y + (haken.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Für diesen Kodex nicht mehr anzeigen".to_string(), x: haken.x + CHECK_SIZE + 10.0, y: ty, color: theme::text(), scale: UI_SCALE_SMALL });
        for (r, text, farbe) in [(trennen, "Trennen", g(Slot::Surface2)), (annehmen, "Annehmen", g(Slot::Accent))] {
            let b = Button { slider: None, action: ButtonAction::Nothing, rect: r, label: String::new(), enabled: true, color: farbe, scale: UI_SCALE_SMALL };
            let (flaeche, tf) = styled_button(&b, self.hover_level(r, r.contains(cx, cy)));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, text.to_string(), tf, UI_SCALE_SMALL));
        }
    }

    fn code_of_conduct_press(&mut self) {
        let Some((_, _, haken, trennen, annehmen, _)) = self.gpu.as_ref().and_then(|g| self.code_of_conduct_layout(g)) else { return };
        let (cx, cy) = self.cursor_pos;
        if haken.contains(cx, cy) {
            if let Some(c) = self.code_of_conduct.as_mut() {
                c.dont_show_again = !c.dont_show_again;
            }
            self.play_ui_sound(ui_sound::UiSound::Click);
        } else if annehmen.contains(cx, cy) {
            let Some(c) = self.code_of_conduct.take() else { return };
            self.play_ui_sound(ui_sound::UiSound::Click);
            if let Some(tx) = &self.outgoing_tx {
                let _ = tx.send(protocol::packets::accept_code_of_conduct_payload());
            }
            // Gemerkt wird nur mit Haken; ohne wird eine fruehere Annahme vergessen
            // (CodeOfConductScreen.onResult).
            let mut liste = load_conduct();
            liste.retain(|(a, _)| *a != self.server_address);
            if c.dont_show_again {
                liste.push((self.server_address.clone(), dialog::java_hash(&c.text)));
            }
            save_conduct(&liste);
        } else if trennen.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.code_of_conduct = None;
            self.disconnect();
            self.disconnect_popup = Some(vanilla_text("multiplayer.disconnect.code_of_conduct", "Der Server verlangt, dass du seinen Verhaltenskodex annimmst."));
        }
    }

    // Pausemenue: "Server-Links" zeigt die Links des Servers als Dialog (Vanillas
    // server_links-Dialog mit Zurueck-Knopf).
    pub(crate) fn open_server_links(&mut self) {
        let zurueck = ActionButton { label: dialog::plain_spans("Zurück"), tooltip: None, width: 200.0, action: None };
        let d = Dialog {
            title: dialog::plain_spans(&vanilla_text("menu.server_links.title", "Server-Links")),
            external_title: None,
            can_close_with_escape: true,
            after_action: AfterAction::Close,
            body: Vec::new(),
            inputs: Vec::new(),
            kind: Kind::ServerLinks { exit: Some(zurueck), columns: 2, button_width: 150.0 },
        };
        self.open_dialog(d);
    }
}

// "%s: %s" mit Beschriftung und Wert (auch "%1$s"/"%2$s").
fn format_label(format_key: &str, name: &str, wert: &str) -> String {
    let muster = vanilla_text(format_key, "%s: %s");
    let mut out = muster.replace("%1$s", name).replace("%2$s", wert);
    if let Some(i) = out.find("%s") {
        out.replace_range(i..i + 2, name);
    }
    if let Some(i) = out.find("%s") {
        out.replace_range(i..i + 2, wert);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beschriftung_des_reglers() {
        assert_eq!(format_label("gibt.es.nicht", "Menge", "5"), "Menge: 5");
    }
}
