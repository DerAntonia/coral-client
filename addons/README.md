# Addons für Coral Client

Ein Addon ist ein WebAssembly-Modul (`.wasm`), das der Client in seinen eigenen Prozess lädt. Ähnlich wie ein Fabric-Mod hängt es sich an feste Stellen im Client, die **Hooks**: jeder Tick, jedes Bild, jede Chatzeile, jedes Paket, jede Taste. An vielen dieser Stellen darf es eingreifen, also das Ereignis verwerfen oder seinen Inhalt ersetzen. Dabei liest es den Spielzustand und löst Aktionen aus.

Jedes Addon bekommt im Modul-Menü eine eigene Kategorie. Seine Module lassen sich dort wie die eingebauten an- und ausschalten und einstellen, und der Client speichert ihre Einstellungen mit der Config.

Mixins im Sinne von Fabric, also das Umschreiben beliebiger Funktionen zur Laufzeit, gibt es nicht. Das geht nur mit Java-Bytecode, nicht mit einem kompilierten Programm. Stattdessen bietet der Client die Hooks und Aufrufe unten an. Fehlt dir eine Stelle, dann melde sie: Neue Hooks lassen sich ergänzen, ohne dass bestehende Addons brechen.

## Installieren

Lege die `.wasm`-Datei in den Ordner `mods`. Dieselbe Datei läuft unter Linux und Windows.

- Linux: `~/coral-client/mods/`
- Windows: `%USERPROFILE%\coral-client\mods\`

Der Client lädt beim Start jede `.wasm`-Datei aus diesem Ordner. Ob es geklappt hat, steht im Log auf dem Connect-Screen, zum Beispiel „Addon beispiel_addon angemeldet (3 Module)“.

## Template

[`template/`](template/) ist ein lauffähiges Addon in Rust. Es benutzt jeden Hook, jede Art von Einstellung und jeden Aufruf einmal, und zwar in drei Modulen:

- **Info-HUD**: Position, Blickrichtung, Ziel, nächster Spieler, Pakete pro Sekunde, FPS und das Item in der Hand.
- **Chat-Werkzeuge**: ersetzt `:shrug:`, spielt einen Ton bei Erwähnung, blendet Discord-Links aus und bietet den Befehl `/beispiel`.
- **Steuerung**: Taste R schaltet automatisches Laufen um, die mittlere Maustaste wählt ein Totem aus der Hotbar.

```sh
rustup target add wasm32-unknown-unknown
cd addons/template
cargo build --release
cp target/wasm32-unknown-unknown/release/beispiel_addon.wasm ~/coral-client/mods/
```

Ein eigenes Addon fängt man am einfachsten mit einer Kopie des Templates an und löscht, was man nicht braucht.

## Aufbau

```rust
use coral_addon::*;

#[derive(Default)]
struct MeinAddon { /* eigener Zustand */ }

impl Addon for MeinAddon {
    fn register(&mut self) -> Registration {
        Registration {
            name: "Mein Addon".into(),          // Kategorie im Modul-Menü
            hooks: vec![Hook::Tick, Hook::ChatIn],
            modules: vec![Module { id: "mein_modul".into(), name: "Mein Modul".into(), description: "…".into(), settings: vec![] }],
            commands: vec!["mein".into()],      // /mein … geht an das Addon statt an den Server
        }
    }

    fn on_event(&mut self, event: Event) -> Flow {
        match event {
            Event::ChatIn { text } if text.contains("Spam") => Flow::Cancel,
            Event::Tick => { let p = client::player(); /* … */ Flow::Continue }
            _ => Flow::Continue,
        }
    }
}

export!(MeinAddon);
```

`Cargo.toml` braucht `crate-type = ["cdylib"]` und die Abhängigkeit `coral-addon`. Wie das aussieht, steht im Template.

## Ablauf

1. Der Client lädt das Modul und ruft `register` auf.
2. Danach schickt er den gespeicherten Stand: `ModuleEnabled` für jedes Modul und `Setting` für jede Einstellung.
3. Ab dann ruft er `on_event` an jedem angemeldeten Hook auf und bei jeder Änderung im Menü.

Nur angemeldete Hooks kosten überhaupt Zeit. Ein Addon, das nur `Tick` hört, wird nur 20-mal pro Sekunde aufgerufen.

Ein Addon mit Modulen ist nur an, solange mindestens eines davon an ist. Sind alle aus, bekommt es keine Hooks und keine Befehle mehr; seine Befehle gehen dann an den Server, und `set_input` zählt nicht. `ModuleEnabled`, `Setting` und `Button` kommen weiter an. Ein Addon ohne Module ist immer an.

Mehrere Addons laufen nacheinander, in alphabetischer Reihenfolge der Dateinamen. `Flow::Cancel` beendet die Kette. Ein ersetzter Inhalt geht an das nächste Addon weiter.

## Hooks

| Hook | Event | Eingreifen |
|---|---|---|
| `Tick` | `Tick`, vor jedem Spieler-Tick (20 pro Sekunde, nur verbunden) | – |
| `Frame` | `Frame { dt }`, einmal pro Durchlauf der Hauptschleife | – |
| `Join` / `Leave` | `Join { server }` / `Leave { reason }` | – |
| `Death` | `Death`, der eigene Spieler ist gestorben | – |
| `ChatIn` | `ChatIn { text }`, eine Chatzeile vom Server, ohne Farben | `Cancel`, `ReplaceText` |
| `ChatOut` | `ChatOut { text }`, Chatzeile oder Befehl des Spielers | `Cancel`, `ReplaceText` |
| `PacketIn` | `PacketIn { id, data }`, jedes Play-Paket vom Server, Packet-ID vorne in `data` | nur lesen |
| `PacketOut` | `PacketOut { id, data }`, jedes Paket an den Server | `Cancel`, `ReplaceBytes` |
| `Key` | `Key { key, pressed }`, Steuertaste in der Welt, Name wie `"KeyW"`, `"Space"`, `"F5"`, `"ShiftLeft"` | `Cancel` |
| `Mouse` | `Mouse { button, pressed }`: 0 links, 1 rechts, 2 Mitte, 3 zurück, 4 vor | `Cancel` |
| `Scroll` | `Scroll { delta }`, eine Raste des Mausrads | `Cancel` |

Diese Events kommen ohne Anmeldung:

- `ModuleEnabled { module, on }`
- `Setting { module, key, value }`
- `Button { module, key }`
- `Command { name, args }`, für die eigenen Befehle

`Cancel` bei `Key`, `Mouse` oder `Scroll` heißt: Das Spiel sieht die Eingabe nicht. Tippen in Chat und Menüs läuft nicht über diese Hooks.

Hört ein Addon `PacketOut`, hält der Client ausgehende Pakete kurz zurück, bis sie durch den Hook gelaufen sind. Das kostet höchstens einen Durchlauf der Hauptschleife. `PacketIn` kann ein Paket nicht verwerfen, weil es im Netzwerk-Thread schon verarbeitet wird.

## Aufrufe (`client::*`)

Abfragen antworten sofort mit dem Stand in diesem Moment. Aktionen führt der Client direkt nach dem laufenden Hook aus, in der Reihenfolge der Aufrufe.

**Abfragen**

| Funktion | Ergebnis |
|---|---|
| `player()` | Position, Geschwindigkeit, Blick, Leben, Hunger, Sättigung, XP, Spielmodus, Hotbar-Slot, Dimension, Server, Name, Entity-ID. `in_world` ist false im Menü |
| `inventory()` | 46 Slots in Vanilla-Reihenfolge: 0 Ergebnis, 1–4 Crafting, 5–8 Rüstung, 9–35 Inventar, 36–44 Hotbar, 45 Zweithand |
| `entities(radius)` | Entities im Umkreis, nächste zuerst: ID, Art (`"minecraft:zombie"`), Name, Position, Abstand |
| `block(x, y, z)` | `"minecraft:stone"`, `None` bei nicht geladenem Chunk |
| `target()` | Block (mit Seite und Name) oder Entity im Fadenkreuz, sonst `Miss` |
| `players()` | Die Tab-Liste: Name, UUID, Ping, Spielmodus |
| `screen()` | Fenstergröße, HUD-Maßstab, ob ein Bildschirm bzw. der Chat offen ist |
| `setting(module, key)` | Gespeicherter Wert einer eigenen Einstellung |

**Aktionen**

| Funktion | Wirkung |
|---|---|
| `hud(module, items)` | Ersetzt, was das Modul ins HUD zeichnet. Gezeichnet wird nur, solange das Modul an ist |
| `chat(text)` | Chatzeile senden, mit `/` einen Befehl. Höchstens 2 pro Sekunde |
| `print(text)` | Zeile nur im eigenen Chat |
| `notify(title, text)` | Benachrichtigung |
| `sound(event, volume, pitch)` | Vanilla-Klang, z. B. `"block.note_block.pling"` |
| `log(text)` | Zeile im Log |
| `set_setting(module, key, value)` | Eigene Einstellung ändern (ohne Echo als `Setting`) |
| `send_packet(data)` | Fertig kodiertes Paket mit Packet-ID vorne; läuft nicht durch `PacketOut` |
| `set_rotation(yaw, pitch)` | Blickrichtung in Grad wie Vanilla: yaw 0 = Süden, pitch positiv = nach unten |
| `select_slot(slot)` | Hotbar-Slot 0–8 |
| `attack()` / `use_item()` | Wie Links- bzw. Rechtsklick auf das Ziel im Fadenkreuz |
| `drop_item(whole_stack)` / `swap_hands()` | Wie die Tasten Q und F |
| `click_slot(slot, button, mode)` | Klick in das offene Inventar; `mode` wie Vanilla: 0 Klick, 1 Shift-Klick, 2 Hotbar-Tausch, 4 Werfen … |
| `set_input(Input { forward, … })` | Bewegungstasten zusätzlich drücken, bis zum nächsten `set_input`; `Input::default()` lässt los |
| `close_screen()` | Offenen Bildschirm schließen, wie mit Escape |

## Einstellungen

| Konstruktor | Im Menü | Wert im Event |
|---|---|---|
| `Setting::toggle(key, label, default)` | An/Aus-Schalter | `Value::Bool` |
| `Setting::slider(key, label, min, max, step, unit, default)` | Regler, Zahl auch eintippbar | `Value::Number` |
| `Setting::choice(key, label, &[optionen], default)` | Aufklappliste | `Value::Text` (die gewählte Option) |
| `Setting::color(key, label, "#RRGGBB")` | Farbfeld mit Farbwähler | `Value::Text` (`"#RRGGBB"`) |
| `Setting::button(key, label)` | Knopf; ein Klick schickt `Event::Button` | – |

Modul-IDs und Schlüssel dürfen nur `a-z`, `0-9`, `_` und `-` enthalten; andere Zeichen werden zu `_`. Über sie speichert der Client die Werte. Ändere sie deshalb nach dem Veröffentlichen nicht mehr.

Eine Auswahl liefert den angezeigten Text der Option. Benennst du eine Option um, passt ein gespeicherter alter Wert nicht mehr.

## HUD

`HudItem`-Elemente werden in GUI-Einheiten angegeben, also mit dem HUD-Maßstab multipliziert. Sie zählen von ihrem `anchor` aus nach innen: `TopLeft`, `TopRight`, `BottomLeft`, `BottomRight` oder `Center`. Farben werden als `0xRRGGBB` angegeben.

- `Text { text, x, y, color, anchor }`
- `Rect { x, y, w, h, color, alpha, anchor }`
- `Item { id, count, x, y, anchor }`: Item-Symbol, 16×16

## Grenzen

- Pro Aufruf hat ein Addon ein Rechenbudget von etwa 50 Millionen Instruktionen und insgesamt höchstens 256 MB Speicher.
- Stürzt ein Addon ab, läuft es in eine Endlosschleife oder überschreitet es den Speicher, wird nur dieses Addon abgeschaltet. Der Client meldet es im Log und als Benachrichtigung und läuft weiter.
- Addons haben keinen Zugriff auf Dateien oder das Netz, nur auf die Hooks und Aufrufe oben.

## Versionen

Die Schnittstelle steht in [`crates/addon_api`](../crates/addon_api/src/lib.rs) (`API_VERSION`). Neue Hooks, Events und Aufrufe kommen immer hinten an die Listen. Ein altes Addon läuft deshalb mit einem neuen Client weiter, es bekommt nur die Neuerungen nicht mit.
