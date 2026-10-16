// Server-Dialoge (seit 1.21.6): Fenster, die der Server als NBT beschreibt -- Text, Items,
// Eingabefelder und Knoepfe, deren Aktionen Befehle ausfuehren, Links oeffnen oder die
// Eingaben an den Server zurueckschicken. Hier stehen das Datenmodell, das Einlesen und die
// Werte der Eingaben; gezeichnet und bedient wird in dialog_screen.rs.
//
// Feldnamen und Vorgaben wie Vanillas Codecs (net.minecraft.server.dialog.*): CommonDialogData,
// die fuenf Dialogarten, ActionButton, die Body- und Input-Typen und die Aktionen.

use std::collections::HashMap;

use protocol::nbt::{self, NbtValue, TextSpan};

// Was nach einem Knopfdruck mit dem Fenster passiert (DialogAction).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AfterAction {
    Close,
    None,
    WaitForResponse,
}

// Woher ein Dialog kommt: direkt beschrieben oder als Name in der Dialog-Registry.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Inline(NbtValue),
    Named(String),
}

// Was ein Knopf ausloest. Die ersten sieben sind die Klick-Ereignisse eines Textes
// (StaticAction), die beiden letzten setzen die Eingaben ein (CommandTemplate, CustomAll).
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    OpenUrl(String),
    RunCommand(String),
    SuggestCommand(String),
    CopyToClipboard(String),
    ChangePage,
    ShowDialog(Source),
    Custom { id: String, payload: Option<NbtValue> },
    CommandTemplate(String),
    CustomAll { id: String, additions: Option<HashMap<String, NbtValue>> },
}

// Das fertige Klick-Ereignis, nachdem die Eingaben eingesetzt sind.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    OpenUrl(String),
    RunCommand(String),
    SuggestCommand(String),
    CopyToClipboard(String),
    ShowDialog(Source),
    Custom { id: String, payload: Option<NbtValue> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionButton {
    pub label: Vec<TextSpan>,
    pub tooltip: Option<Vec<TextSpan>>,
    // Breite in GUI-Pixeln (Vanilla: 1..1024, Vorgabe 150).
    pub width: f32,
    pub action: Option<Action>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Text { spans: Vec<TextSpan>, width: f32 },
    Item { id: String, count: i32, description: Option<(Vec<TextSpan>, f32)>, decorations: bool, width: f32, height: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptionEntry {
    pub id: String,
    pub display: Vec<TextSpan>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    Text { width: f32, label: Vec<TextSpan>, label_visible: bool, initial: String, max_length: usize, multiline: Option<(Option<usize>, Option<f32>)> },
    Bool { label: Vec<TextSpan>, initial: bool, on_true: String, on_false: String },
    Choice { width: f32, label: Vec<TextSpan>, label_visible: bool, options: Vec<OptionEntry>, initial: usize },
    Range { width: f32, label: Vec<TextSpan>, label_format: String, start: f32, end: f32, initial: Option<f32>, step: Option<f32> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub key: String,
    pub control: Control,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    // notice: ein Knopf unten (Vorgabe "OK"), Escape loest dessen Aktion aus.
    Notice(ActionButton),
    // confirmation: Ja und Nein unten, Escape wie Nein.
    Confirmation { yes: ActionButton, no: ActionButton },
    // multi_action, server_links, dialog_list: Knoepfe in Spalten im Inhalt, dazu ein
    // optionaler Ausgang unten. Server-Links und Dialog-Liste werden beim Oeffnen zu Knoepfen
    // (s. list_buttons in dialog_screen.rs).
    List { buttons: Vec<ActionButton>, exit: Option<ActionButton>, columns: usize },
    ServerLinks { exit: Option<ActionButton>, columns: usize, button_width: f32 },
    DialogList { dialogs: Vec<Source>, exit: Option<ActionButton>, columns: usize, button_width: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dialog {
    pub title: Vec<TextSpan>,
    pub external_title: Option<Vec<TextSpan>>,
    pub can_close_with_escape: bool,
    pub after_action: AfterAction,
    pub body: Vec<Body>,
    pub inputs: Vec<Input>,
    pub kind: Kind,
}

impl Dialog {
    // Der Titel, unter dem ein anderer Dialog auf diesen verweist (computeExternalTitle).
    pub fn external_title(&self) -> &[TextSpan] {
        self.external_title.as_deref().unwrap_or(&self.title)
    }

    // Was Escape ausloest (onCancel): bei notice der Knopf, bei confirmation Nein, bei den
    // Listen der Ausgang.
    pub fn cancel_action(&self) -> Option<&Action> {
        match &self.kind {
            Kind::Notice(b) => b.action.as_ref(),
            Kind::Confirmation { no, .. } => no.action.as_ref(),
            Kind::List { exit, .. } | Kind::ServerLinks { exit, .. } | Kind::DialogList { exit, .. } => exit.as_ref().and_then(|b| b.action.as_ref()),
        }
    }
}

// --- Einlesen ------------------------------------------------------------------------------

fn flag(v: &NbtValue, key: &str, standard: bool) -> bool {
    match v.get(key) {
        Some(NbtValue::Byte(b)) => *b != 0,
        Some(NbtValue::Int(i)) => *i != 0,
        Some(NbtValue::String(s)) => s == "true",
        _ => standard,
    }
}

fn int(v: &NbtValue, key: &str, standard: i32) -> i32 {
    v.get(key).and_then(NbtValue::as_i32).unwrap_or(standard)
}

fn float(v: &NbtValue, key: &str) -> Option<f32> {
    v.get(key).and_then(NbtValue::as_f32)
}

fn string(v: &NbtValue, key: &str) -> Option<String> {
    v.get(key).and_then(NbtValue::as_str).map(str::to_string)
}

fn text(v: &NbtValue, key: &str) -> Option<Vec<TextSpan>> {
    v.get(key).map(nbt::text_to_spans)
}

// "minecraft:notice" und "notice" meinen dasselbe.
fn type_name(v: &NbtValue) -> Option<&str> {
    let t = v.get("type")?.as_str()?;
    Some(t.strip_prefix("minecraft:").unwrap_or(t))
}

// Ein Wert, der als einzelnes Element oder als Liste stehen darf (compactListCodec).
fn compact_list(v: Option<&NbtValue>) -> Vec<&NbtValue> {
    match v {
        Some(NbtValue::List(items)) => items.iter().collect(),
        Some(other) => vec![other],
        None => Vec::new(),
    }
}

fn width(v: &NbtValue, key: &str, standard: i32) -> f32 {
    int(v, key, standard).clamp(1, 1024) as f32
}

fn source(v: &NbtValue) -> Option<Source> {
    match v {
        NbtValue::String(name) => Some(Source::Named(name.clone())),
        NbtValue::Compound(_) => Some(Source::Inline(v.clone())),
        _ => None,
    }
}

fn action(v: &NbtValue) -> Option<Action> {
    Some(match type_name(v)? {
        "open_url" => Action::OpenUrl(string(v, "url")?),
        "run_command" => Action::RunCommand(string(v, "command")?),
        "suggest_command" => Action::SuggestCommand(string(v, "command")?),
        "copy_to_clipboard" => Action::CopyToClipboard(string(v, "value")?),
        "change_page" => Action::ChangePage,
        "show_dialog" => Action::ShowDialog(source(v.get("dialog")?)?),
        "custom" => Action::Custom { id: string(v, "id")?, payload: v.get("payload").cloned() },
        "dynamic/run_command" => Action::CommandTemplate(string(v, "template")?),
        "dynamic/custom" => Action::CustomAll { id: string(v, "id")?, additions: v.get("additions").and_then(NbtValue::as_compound).cloned() },
        _ => return None,
    })
}

fn button(v: &NbtValue) -> Option<ActionButton> {
    Some(ActionButton { label: text(v, "label")?, tooltip: text(v, "tooltip"), width: width(v, "width", 150), action: v.get("action").and_then(action) })
}

// PlainMessage.CODEC: ein Objekt mit contents/width oder gleich der Text selbst.
fn plain_message(v: &NbtValue) -> (Vec<TextSpan>, f32) {
    match v.get("contents") {
        Some(c) => (nbt::text_to_spans(c), width(v, "width", 200)),
        None => (nbt::text_to_spans(v), 200.0),
    }
}

fn body(v: &NbtValue) -> Option<Body> {
    match type_name(v)? {
        "plain_message" => {
            let (spans, width) = plain_message(v);
            Some(Body::Text { spans, width })
        }
        "item" => {
            let item = v.get("item")?;
            let id = match item {
                NbtValue::String(s) => s.clone(),
                _ => string(item, "id")?,
            };
            Some(Body::Item {
                id,
                count: int(item, "count", 1),
                description: v.get("description").map(plain_message),
                decorations: flag(v, "show_decorations", true),
                width: int(v, "width", 16).clamp(1, 256) as f32,
                height: int(v, "height", 16).clamp(1, 256) as f32,
            })
        }
        _ => None,
    }
}

// Ein Eintrag von single_option: voll ({id, display, initial}) oder nur die ID als String.
fn option_entry(v: &NbtValue) -> Option<(OptionEntry, bool)> {
    match v {
        NbtValue::String(id) => Some((OptionEntry { id: id.clone(), display: plain_spans(id) }, false)),
        _ => {
            let id = string(v, "id")?;
            let display = text(v, "display").unwrap_or_else(|| plain_spans(&id));
            Some((OptionEntry { id, display }, flag(v, "initial", false)))
        }
    }
}

pub fn plain_spans(text: &str) -> Vec<TextSpan> {
    vec![TextSpan { text: text.to_string(), ..Default::default() }]
}

// StringTemplate.isValidVariableName: Buchstaben, Ziffern, Unterstrich.
fn valid_key(key: &str) -> bool {
    key.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn input(v: &NbtValue) -> Option<Input> {
    let key = string(v, "key").filter(|k| valid_key(k))?;
    let control = match type_name(v)? {
        "text" => {
            let max_length = int(v, "max_length", 32).max(1) as usize;
            let multiline = v.get("multiline").map(|m| (m.get("max_lines").and_then(NbtValue::as_i32).map(|n| n.max(1) as usize), float(m, "height")));
            Control::Text {
                width: width(v, "width", 200),
                label: text(v, "label")?,
                label_visible: flag(v, "label_visible", true),
                initial: string(v, "initial").unwrap_or_default().chars().take(max_length).collect(),
                max_length,
                multiline,
            }
        }
        "boolean" => Control::Bool {
            label: text(v, "label")?,
            initial: flag(v, "initial", false),
            on_true: string(v, "on_true").unwrap_or_else(|| "true".into()),
            on_false: string(v, "on_false").unwrap_or_else(|| "false".into()),
        },
        "single_option" => {
            let eintraege: Vec<(OptionEntry, bool)> = compact_list(v.get("options")).into_iter().filter_map(option_entry).collect();
            if eintraege.is_empty() {
                return None;
            }
            let initial = eintraege.iter().position(|(_, i)| *i).unwrap_or(0);
            Control::Choice {
                width: width(v, "width", 200),
                label: text(v, "label")?,
                label_visible: flag(v, "label_visible", true),
                options: eintraege.into_iter().map(|(e, _)| e).collect(),
                initial,
            }
        }
        "number_range" => Control::Range {
            width: width(v, "width", 200),
            label: text(v, "label")?,
            label_format: string(v, "label_format").unwrap_or_else(|| "options.generic_value".into()),
            start: float(v, "start")?,
            end: float(v, "end")?,
            initial: float(v, "initial"),
            step: float(v, "step").filter(|s| *s > 0.0),
        },
        _ => return None,
    };
    Some(Input { key, control })
}

fn columns(v: &NbtValue) -> usize {
    int(v, "columns", 2).max(1) as usize
}

// Einen Dialog aus seiner NBT-Beschreibung lesen. None, wenn Pflichtfelder fehlen oder die
// Art unbekannt ist -- dann zeigt auch Vanilla nichts.
pub fn parse(v: &NbtValue) -> Option<Dialog> {
    let kind = match type_name(v)? {
        "notice" => Kind::Notice(v.get("action").and_then(button).unwrap_or_else(|| ActionButton {
            label: plain_spans(&crate::vanilla_text("gui.ok", "Ok")),
            tooltip: None,
            width: 150.0,
            action: None,
        })),
        "confirmation" => Kind::Confirmation { yes: button(v.get("yes")?)?, no: button(v.get("no")?)? },
        "multi_action" => {
            let buttons: Vec<ActionButton> = compact_list(v.get("actions")).into_iter().filter_map(button).collect();
            if buttons.is_empty() {
                return None;
            }
            Kind::List { buttons, exit: v.get("exit_action").and_then(button), columns: columns(v) }
        }
        "server_links" => Kind::ServerLinks { exit: v.get("exit_action").and_then(button), columns: columns(v), button_width: width(v, "button_width", 150) },
        "dialog_list" => Kind::DialogList {
            dialogs: compact_list(v.get("dialogs")).into_iter().filter_map(source).collect(),
            exit: v.get("exit_action").and_then(button),
            columns: columns(v),
            button_width: width(v, "button_width", 150),
        },
        _ => return None,
    };
    let after_action = match string(v, "after_action").as_deref() {
        Some("none") => AfterAction::None,
        Some("wait_for_response") => AfterAction::WaitForResponse,
        _ => AfterAction::Close,
    };
    Some(Dialog {
        title: text(v, "title")?,
        external_title: text(v, "external_title"),
        can_close_with_escape: flag(v, "can_close_with_escape", true),
        after_action,
        body: compact_list(v.get("body")).into_iter().filter_map(body).collect(),
        inputs: compact_list(v.get("inputs")).into_iter().filter_map(input).collect(),
        kind,
    })
}

// --- Werte der Eingaben ----------------------------------------------------------------------

// Der aktuelle Stand einer Eingabe. Der Text selbst steht im Textfeld des Fensters, hier nur
// sein Inhalt (s. dialog_screen.rs).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Bool(bool),
    Choice(usize),
    // Reglerstellung 0..1.
    Range(f32),
}

impl Control {
    pub fn initial_value(&self) -> Value {
        match self {
            Control::Text { initial, .. } => Value::Text(initial.clone()),
            Control::Bool { initial, .. } => Value::Bool(*initial),
            Control::Choice { initial, .. } => Value::Choice(*initial),
            Control::Range { start, end, initial, .. } => Value::Range(slider_of(*start, *end, initial.unwrap_or((start + end) / 2.0))),
        }
    }
}

// NumberRangeInput.RangeInfo.scaledValueToSlider.
fn slider_of(start: f32, end: f32, wert: f32) -> f32 {
    if start == end {
        0.5
    } else {
        (wert - start) / (end - start)
    }
}

// RangeInfo.computeScaledValue: die Reglerstellung als Zahl, mit Schrittweite vom
// Anfangswert aus gerastert; ein Raster ausserhalb des Bereichs faellt einen Schritt zurueck.
pub fn range_value(start: f32, end: f32, initial: Option<f32>, step: Option<f32>, slider: f32) -> f32 {
    let wert = start + (end - start) * slider;
    let Some(step) = step else { return wert };
    let basis = initial.unwrap_or((start + end) / 2.0);
    let n = ((wert - basis) / step).round();
    let gerastert = basis + n * step;
    let s = slider_of(start, end, gerastert);
    if (0.0..=1.0).contains(&s) {
        gerastert
    } else {
        basis + (n - n.signum()) * step
    }
}

// Javas Float.toString fuer die Werte, die vorkommen: ganze Zahlen ohne Nachkomma, sonst die
// kuerzeste Darstellung.
pub fn number_text(v: f32) -> String {
    if v == v.trunc() && v.abs() < 1e9 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

// StringTag.escapeWithoutQuotes: Anfuehrungszeichen und Backslash mit Backslash davor,
// Steuerzeichen als Escape-Folge.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' | '\'' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

impl Input {
    // Was in eine Befehlsvorlage eingesetzt wird (ValueGetter.asTemplateSubstitution).
    pub fn substitution(&self, wert: &Value) -> String {
        match (&self.control, wert) {
            (Control::Text { .. }, Value::Text(t)) => escape(t),
            (Control::Bool { on_true, on_false, .. }, Value::Bool(b)) => if *b { on_true.clone() } else { on_false.clone() },
            (Control::Choice { options, .. }, Value::Choice(i)) => options.get(*i).map(|o| o.id.clone()).unwrap_or_default(),
            (Control::Range { start, end, initial, step, .. }, Value::Range(s)) => number_text(range_value(*start, *end, *initial, *step, *s)),
            _ => String::new(),
        }
    }

    // Was an den Server geht (ValueGetter.asTag): Text als String, Schalter als Byte, Auswahl
    // als ID, Zahl als Float.
    pub fn tag(&self, wert: &Value) -> NbtValue {
        match (&self.control, wert) {
            (Control::Text { .. }, Value::Text(t)) => NbtValue::String(t.clone()),
            (Control::Bool { .. }, Value::Bool(b)) => NbtValue::Byte(*b as i8),
            (Control::Range { start, end, initial, step, .. }, Value::Range(s)) => NbtValue::Float(range_value(*start, *end, *initial, *step, *s)),
            _ => NbtValue::String(self.substitution(wert)),
        }
    }
}

// StringTemplate: jedes "$(name)" wird durch den Wert der Eingabe ersetzt, unbekannte Namen
// durch nichts. Ein "$" ohne Klammer bleibt stehen.
pub fn fill_template(template: &str, werte: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find("$(") {
        let Some(ende) = rest[i + 2..].find(')') else { break };
        out.push_str(&rest[..i]);
        let name = &rest[i + 2..i + 2 + ende];
        out.push_str(werte.get(name).map_or("", String::as_str));
        rest = &rest[i + 2 + ende + 1..];
    }
    out.push_str(rest);
    out
}

impl Action {
    // Das Klick-Ereignis mit den eingesetzten Werten (Action.createAction). ChangePage tut in
    // einem Dialog nichts.
    pub fn event(&self, inputs: &[Input], werte: &[Value]) -> Option<Event> {
        Some(match self {
            Action::OpenUrl(u) => Event::OpenUrl(u.clone()),
            Action::RunCommand(c) => Event::RunCommand(c.clone()),
            Action::SuggestCommand(c) => Event::SuggestCommand(c.clone()),
            Action::CopyToClipboard(v) => Event::CopyToClipboard(v.clone()),
            Action::ChangePage => return None,
            Action::ShowDialog(s) => Event::ShowDialog(s.clone()),
            Action::Custom { id, payload } => Event::Custom { id: id.clone(), payload: payload.clone() },
            Action::CommandTemplate(t) => {
                let ersatz: HashMap<String, String> = inputs.iter().zip(werte).map(|(i, w)| (i.key.clone(), i.substitution(w))).collect();
                Event::RunCommand(fill_template(t, &ersatz))
            }
            Action::CustomAll { id, additions } => {
                let mut map = additions.clone().unwrap_or_default();
                for (i, w) in inputs.iter().zip(werte) {
                    map.insert(i.key.clone(), i.tag(w));
                }
                Event::Custom { id: id.clone(), payload: Some(NbtValue::Compound(map)) }
            }
        })
    }
}

// Javas String.hashCode -- Vanilla merkt sich einen angenommenen Verhaltenskodex nur als
// diesen Hash (ServerData.acceptCodeOfConduct).
pub fn java_hash(text: &str) -> i32 {
    text.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> NbtValue {
        NbtValue::String(t.to_string())
    }

    fn c(felder: &[(&str, NbtValue)]) -> NbtValue {
        NbtValue::Compound(felder.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }

    #[test]
    fn notice_mit_vorgaben() {
        let d = parse(&c(&[("type", s("minecraft:notice")), ("title", s("Hallo")), ("body", c(&[("type", s("plain_message")), ("contents", s("Text"))]))])).unwrap();
        assert!(d.can_close_with_escape);
        assert_eq!(d.after_action, AfterAction::Close);
        assert_eq!(d.body, vec![Body::Text { spans: plain_spans("Text"), width: 200.0 }]);
        assert!(matches!(d.kind, Kind::Notice(ActionButton { action: None, width: 150.0, .. })));
    }

    #[test]
    fn unbekannte_art_oder_fehlender_titel_zeigt_nichts() {
        assert!(parse(&c(&[("type", s("minecraft:quiz")), ("title", s("x"))])).is_none());
        assert!(parse(&c(&[("type", s("minecraft:notice"))])).is_none());
    }

    #[test]
    fn befehlsvorlage_setzt_die_eingaben_ein() {
        let d = parse(&c(&[
            ("type", s("minecraft:multi_action")),
            ("title", s("Kauf")),
            (
                "inputs",
                NbtValue::List(vec![
                    c(&[("type", s("minecraft:text")), ("key", s("name")), ("label", s("Name"))]),
                    c(&[("type", s("minecraft:boolean")), ("key", s("ja")), ("label", s("Ja?")), ("initial", NbtValue::Byte(1))]),
                    c(&[("type", s("minecraft:single_option")), ("key", s("farbe")), ("label", s("Farbe")), ("options", NbtValue::List(vec![s("rot"), c(&[("id", s("blau")), ("initial", NbtValue::Byte(1))])]))]),
                ]),
            ),
            ("actions", NbtValue::List(vec![c(&[("label", s("Los")), ("action", c(&[("type", s("dynamic/run_command")), ("template", s("kauf $(name) $(ja) $(farbe) $(fehlt)$"))]))])])),
        ]))
        .unwrap();
        let mut werte: Vec<Value> = d.inputs.iter().map(|i| i.control.initial_value()).collect();
        assert_eq!(werte[2], Value::Choice(1));
        werte[0] = Value::Text("Ste\"ve".into());
        let Kind::List { buttons, .. } = &d.kind else { panic!() };
        let e = buttons[0].action.as_ref().unwrap().event(&d.inputs, &werte);
        assert_eq!(e, Some(Event::RunCommand("kauf Ste\\\"ve true blau $".into())));
    }

    #[test]
    fn custom_all_schickt_die_werte_als_tags() {
        let inputs = vec![
            Input { key: "n".into(), control: Control::Range { width: 200.0, label: vec![], label_format: String::new(), start: 0.0, end: 10.0, initial: Some(2.0), step: Some(1.0) } },
            Input { key: "b".into(), control: Control::Bool { label: vec![], initial: false, on_true: "t".into(), on_false: "f".into() } },
        ];
        let werte = vec![Value::Range(0.53), Value::Bool(true)];
        let mut extra = HashMap::new();
        extra.insert("fest".to_string(), s("x"));
        let e = Action::CustomAll { id: "a:b".into(), additions: Some(extra) }.event(&inputs, &werte).unwrap();
        let Event::Custom { id, payload: Some(NbtValue::Compound(map)) } = e else { panic!() };
        assert_eq!(id, "a:b");
        assert_eq!(map["n"], NbtValue::Float(5.0));
        assert_eq!(map["b"], NbtValue::Byte(1));
        assert_eq!(map["fest"], s("x"));
    }

    #[test]
    fn regler_rastet_vom_anfangswert_aus() {
        assert_eq!(range_value(0.0, 10.0, Some(1.5), Some(2.0), 0.5), 5.5);
        // Am Ende gerundet waere 11 ueber dem Bereich: einen Schritt zurueck.
        assert_eq!(range_value(0.0, 10.0, Some(1.0), Some(2.0), 1.0), 9.0);
        assert_eq!(range_value(0.0, 10.0, None, None, 0.25), 2.5);
        assert_eq!(number_text(5.0), "5");
        assert_eq!(number_text(2.5), "2.5");
    }

    #[test]
    fn hash_wie_java() {
        assert_eq!(java_hash(""), 0);
        assert_eq!(java_hash("abc"), 96354);
        assert_eq!(java_hash("Hallo Welt"), "Hallo Welt".encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32)));
    }
}
