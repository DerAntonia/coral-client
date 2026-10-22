// Beispiel-Addon fuer Coral Client. Bauen mit `cargo build --release`, dann die Datei
// target/release/beispiel-addon (Windows: beispiel-addon.exe) nach ~/coral-client/mods
// kopieren und den Client neu starten. Im Modul-Menue erscheint die Kategorie "Beispiel".
//
// Das Protokoll (addons/README.md): eine JSON-Nachricht je Zeile, vom Client auf stdin, an den
// Client auf stdout. Alles auf stderr landet im Log des Clients -- dorthin also Fehlersuche,
// NIE auf stdout.

use serde_json::{json, Value};
use std::io::{BufRead, Write};

// Was dieses Addon sich merkt.
struct Stand {
    an: bool,
    gruss: bool,
    abstand: f64,
    ecke: String,
    farbe: String,
    x: f64,
    y: f64,
    z: f64,
}

fn senden(msg: Value) {
    let mut out = std::io::stdout().lock();
    // Bricht die Verbindung ab, ist der Client weg -- dann einfach aufhoeren.
    if writeln!(out, "{msg}").and_then(|_| out.flush()).is_err() {
        std::process::exit(0);
    }
}

fn anmelden() {
    senden(json!({
        "type": "register",
        "name": "Beispiel",
        "subscribe": ["tick", "chat", "join", "leave"],
        "modules": [{
            "id": "koordinaten",
            "name": "Koordinaten",
            "description": "Zeigt die eigene Position im HUD",
            "settings": [
                { "type": "toggle", "key": "gruss", "label": "Gruß beim Betreten", "default": false },
                { "type": "slider", "key": "abstand", "label": "Abstand zum Rand", "min": 0, "max": 50, "step": 1, "unit": "px", "default": 4 },
                { "type": "choice", "key": "ecke", "label": "Ecke", "options": ["oben links", "oben rechts", "unten links", "unten rechts"], "default": "oben links" },
                { "type": "color", "key": "farbe", "label": "Textfarbe", "default": "#7FE3FF" },
                { "type": "button", "key": "test", "label": "Test-Benachrichtigung" }
            ]
        }]
    }));
}

fn zeichnen(s: &Stand) {
    let items = if s.an {
        let anker = match s.ecke.as_str() {
            "oben rechts" => "top_right",
            "unten links" => "bottom_left",
            "unten rechts" => "bottom_right",
            _ => "top_left",
        };
        let text = format!("X {:.1}  Y {:.1}  Z {:.1}", s.x, s.y, s.z);
        json!([{ "kind": "text", "text": text, "x": s.abstand, "y": s.abstand, "anchor": anker, "color": s.farbe }])
    } else {
        json!([])
    };
    senden(json!({ "type": "hud", "module": "koordinaten", "items": items }));
}

fn main() {
    let mut s = Stand { an: false, gruss: false, abstand: 4.0, ecke: "oben links".into(), farbe: "#7FE3FF".into(), x: 0.0, y: 0.0, z: 0.0 };
    // Der Client schliesst stdin, wenn er beendet wird: dann endet diese Schleife und mit ihr
    // das Addon.
    for zeile in std::io::stdin().lock().lines() {
        let Ok(zeile) = zeile else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&zeile) else { continue };
        match msg["type"].as_str().unwrap_or_default() {
            "hello" => anmelden(),
            "enabled" => {
                s.an = msg["on"].as_bool().unwrap_or(false);
                zeichnen(&s);
            }
            "setting" => {
                let v = &msg["value"];
                match msg["key"].as_str().unwrap_or_default() {
                    "gruss" => s.gruss = v.as_bool().unwrap_or(false),
                    "abstand" => s.abstand = v.as_f64().unwrap_or(4.0),
                    "ecke" => s.ecke = v.as_str().unwrap_or_default().to_string(),
                    "farbe" => s.farbe = v.as_str().unwrap_or_default().to_string(),
                    _ => {}
                }
                zeichnen(&s);
            }
            "button" => senden(json!({ "type": "notify", "title": "Beispiel", "text": "Der Knopf geht!" })),
            "event" => match msg["event"].as_str().unwrap_or_default() {
                "tick" => {
                    (s.x, s.y, s.z) = (msg["x"].as_f64().unwrap_or(0.0), msg["y"].as_f64().unwrap_or(0.0), msg["z"].as_f64().unwrap_or(0.0));
                    zeichnen(&s);
                }
                "join" if s.an && s.gruss => senden(json!({ "type": "chat", "text": "Hallo!" })),
                "chat" if s.an && msg["text"].as_str().is_some_and(|t| t.ends_with("!ping")) => {
                    senden(json!({ "type": "sound", "event": "block.note_block.pling", "volume": 1.0, "pitch": 1.5 }));
                }
                _ => {}
            },
            other => eprintln!("unbekannte Nachricht: {other}"),
        }
    }
}
