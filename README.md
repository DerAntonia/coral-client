# Addons für Coral Client

Ein Addon ist ein eigenes Programm, das neue Module ins Modul-Menü bringt. Jedes Addon bekommt dort eine eigene Kategorie. Seine Module lassen sich wie die eingebauten an- und ausschalten und einstellen, und der Client speichert die Einstellungen mit der Config.

## Installieren

Lege das fertige Programm in den Ordner `mods`:

- Linux: `~/coral-client/mods/`. Die Datei muss ausführbar sein (`chmod +x`).
- Windows: `%USERPROFILE%\coral-client\mods\`, als `.exe`.

Beim Start des Clients wird jedes Programm in diesem Ordner mitgestartet. Ob es geklappt hat, steht im Log auf dem Connect-Screen, zum Beispiel „Addon beispiel-addon angemeldet (1 Module)“.

## Template

Dieser Branch ist ein lauffähiges Beispiel in Rust ([`src/main.rs`](src/main.rs)). Es zeigt die eigene Position im HUD und nutzt jede Art von Einstellung.

```sh
git clone -b addon-template https://github.com/DerAntonia/coral-client mein-addon
cd mein-addon
cargo build --release
cp target/release/beispiel-addon ~/coral-client/mods/
```

Ein Addon kann in jeder Sprache geschrieben sein, die stdin und stdout lesen und schreiben kann.

## Protokoll (Version 1)

- Jede Nachricht ist ein JSON-Objekt auf einer eigenen Zeile.
- Der Client schreibt auf stdin des Addons, das Addon antwortet auf stdout.
- Was das Addon auf stderr schreibt, landet im Log des Clients. Das ist der Ort für Fehlersuche; stdout ist nur für Protokoll-Nachrichten da.
- Wenn der Client sich beendet, schließt er stdin. Das Addon soll sich dann ebenfalls beenden.
- Zeilen, die kein gültiges JSON sind, schreibt der Client ins Log und ignoriert sie sonst.

### Ablauf

1. Der Client schickt `hello`.
2. Das Addon antwortet mit `register`.
3. Der Client schickt den gespeicherten Stand: `enabled` und `setting` für jedes Modul.
4. Danach schickt der Client Ereignisse und Änderungen, und das Addon kann jederzeit Anfragen stellen.

### Vom Client an das Addon

| `type` | Felder | Bedeutung |
|---|---|---|
| `hello` | `api`, `client` | Protokollversion (1) und Client-Version |
| `enabled` | `module`, `on` | Hauptschalter des Moduls |
| `setting` | `module`, `key`, `value` | Wert einer Einstellung. `toggle` als Wahrheitswert, `slider` als Zahl, `choice` als gewählte Option, `color` als `"#RRGGBB"` |
| `button` | `module`, `key` | Ein Knopf wurde gedrückt |
| `event` | `event`, … | Ein Ereignis, siehe unten. Kommt nur, wenn es in `subscribe` steht |

### Ereignisse

| `event` | Felder | Wann |
|---|---|---|
| `tick` | `in_world`, `health`, `food`, `x`, `y`, `z`, `server` | 20-mal pro Sekunde |
| `chat` | `text` | Eine Chatzeile ist angekommen (als reiner Text, ohne Farben) |
| `join` | `server` | Verbunden mit einem Server |
| `leave` | `reason` | Verbindung getrennt |

### Vom Addon an den Client

**`register`** meldet die Kategorie und ihre Module an:

```json
{"type": "register", "name": "Mein Addon", "subscribe": ["tick", "chat"],
 "modules": [{"id": "mein_modul", "name": "Mein Modul", "description": "Was es tut",
   "settings": [{"type": "toggle", "key": "laut", "label": "Laut", "default": true}]}]}
```

- `name` ist der Name der Kategorie im Modul-Menü.
- `id` und `key` dürfen nur `a-z`, `0-9`, `_` und `-` enthalten; andere Zeichen werden zu `_`. Über sie werden die Werte gespeichert, also nach dem Veröffentlichen nicht mehr ändern.
- Ein erneutes `register` ersetzt die vorherige Anmeldung.

Diese Arten von Einstellungen gibt es:

| `type` | Zusätzliche Felder | Im Menü |
|---|---|---|
| `toggle` | `default`: true/false | An/Aus-Schalter |
| `slider` | `min`, `max`, `step`, `unit`, `default` | Regler, Zahl auch eintippbar |
| `choice` | `options`: Liste von Texten, `default`: eine davon | Aufklappliste |
| `color` | `default`: `"#RRGGBB"` | Farbfeld mit Farbwähler |
| `button` | – | Knopf; ein Klick schickt `button` |

Jede Einstellung hat außerdem `key` und `label`.

**Weitere Anfragen:**

| `type` | Felder | Wirkung |
|---|---|---|
| `hud` | `module`, `items` | Ersetzt, was dieses Modul ins HUD zeichnet. Gezeichnet wird nur, solange das Modul an ist |
| `chat` | `text` | Sendet eine Chatzeile oder mit `/` einen Befehl. Höchstens 2 pro Sekunde, mehr wird verworfen |
| `notify` | `title`, `text` | Zeigt eine Benachrichtigung |
| `sound` | `event`, `volume`, `pitch` | Spielt einen Vanilla-Klang ab, z. B. `"block.note_block.pling"` |
| `log` | `text` | Schreibt ins Log |
| `set` | `module`, `key`, `value` | Ändert eine eigene Einstellung (der Client meldet sie nicht zurück) |

Elemente in `hud.items`:

```json
{"kind": "text", "text": "Hallo", "x": 4, "y": 4, "anchor": "top_left", "color": "#FFFFFF"}
{"kind": "rect", "x": 2, "y": 2, "w": 60, "h": 12, "anchor": "top_left", "color": "#000000", "alpha": 0.5}
```

- `anchor` ist einer von `top_left`, `top_right`, `bottom_left`, `bottom_right` oder `center`.
- `x` und `y` zählen vom Anker aus nach innen, in GUI-Einheiten. Sie werden also mit der HUD-Skalierung multipliziert.
