# Coral Client

[English](README.md) · **Deutsch**

Ein eigener Minecraft-Java-Client, von Grund auf in Rust geschrieben. Er ist für PvP gebaut:
Zwischen Maus, Bild und Ton soll so wenig Verzögerung wie möglich liegen.

Er spricht das normale Minecraft-Protokoll und verbindet sich mit ganz normalen Servern. Auf dem
Server braucht es dafür keinen Mod und kein Plugin. Spielmaterial wie Texturen, Klänge und Modelle
liefert er nicht mit, er liest es aus deiner eigenen Minecraft-Installation.

**Zielversion: Minecraft Java 1.21.11.**

## Installation

### Was du brauchst

- Ein gekauftes Minecraft: Java Edition. Angemeldet wird mit deinem Microsoft-Konto.
- Minecraft **1.21.11**, einmal installiert. Das geht über den offiziellen Launcher, den Prism
  Launcher oder einen eigenen Ordner.
- Linux oder Windows (64 Bit) und eine Grafikkarte mit Vulkan. Unter Windows geht auch
  DirectX 12.

### Mit dem Launcher

1. Unter [Releases](../../releases) den Launcher für dein System herunterladen
   (`coral-client-launcher-linux-x86_64` oder `coral-client-launcher-windows-x86_64.exe`).
2. Ihn starten (unter Linux vorher ausführbar machen) und auf **Client herunterladen** drücken.
3. Im Launcher die Minecraft-Installation wählen und auf **Spielen** drücken.

Der Launcher sucht selbst nach Updates. Beta-Versionen zeigt er erst an, wenn
„Beta-Versionen anzeigen“ eingeschaltet ist.

## Funktionen

### Latenz

- Rohe Mauseingabe, einstellbarer Present-Mode und Steuerung, wo im Bild ein Riss liegen darf.
- Bewegung, Essen, Kristalle und gesetzte Blöcke sagt der Client selbst voraus, statt auf den
  Server zu warten.
- Der Ton geht mit kleinen Puffern direkt an die Ausgabe, denn Klänge zählen im PvP genauso wie
  das Bild.

### Grafik

- **PvP-Renderpfad:** schnell und schlicht, auf hohe Bildraten ausgelegt.
- **Resourcepacks** mit eigenen Item-Modellen, Core-Shadern, HD-Inventaren und Himmeln im
  OptiFine-Format.
- **Fernsicht wie Voxy:** Der Chunk-Saver speichert jeden geladenen Chunk. Daraus zeichnet er
  Landschaft weit über die Render-Distance hinaus.
- **Partikel** lassen sich für jede Art einzeln ein- und ausschalten und in Dauer, Deckkraft
  und Tempo einstellen.

### Module

Jedes Modul lässt sich im Modul-Menü einzeln ein- und ausschalten. Die Gruppen entsprechen denen
im Spiel.

**Crystal PvP**
- **Totem-Anzeige:** zeigt, ob in Hotbar und Zweithand ein Totem liegt.
- **Totem-Warnung:** zeigt ein X am Fadenkreuz, wenn ein Totem nicht an seinem Platz liegt.
- **Totem-Vorschau:** zeigt schon vor dem Öffnen des Inventars, wo die Totems liegen.
- **Pop-Klänge:** eigene Klänge für Pops, Explosionen und Anker.
- **Pop-Optik:** schaltet das fliegende Totem und die Partikelwolke beim Pop ab.
- **Crystal-Optimizer:** getroffene Kristalle verschwinden sofort, ohne auf den Server zu warten.

**Render**
- **Hitbox-Anzeige:** Trefferboxen anderer Entities, geglättet und/oder roh.
- **Schilde:** Cooldown des eigenen und fremder Schilde und gehobene Schilde anderer Spieler.
- **Timer:** ein Kreis am Fadenkreuz. Beim Essen füllt er sich, bis man fertig ist. Beim
  Bogenspannen leert er sich, bis der Pfeil kritisch ist.
- **Pfeile:** Munitionsanzeige bei Bogen und Armbrust und weiße Flugbahnen hinter Pfeilen.
- **Himmel:** einfarbig, als Verlauf mit Sonne und Mond oder der Himmel aus dem Pack. Dazu
  eine feste Uhrzeit.
- **Wetter:** Regen, Schnee oder klar, fest eingestellt und in einstellbarem Abstand um dich.
  Das betrifft nur das Bild.
- **Farbe:** Sättigung des ganzen Bildes.
- **Blockumriss:** Farbe, Deckkraft und Breite des Rahmens um den anvisierten Block.
- **Dynamisches Feuer:** mit Feuerresistenz blenden Feuerblöcke aus.
- **Chunk-Saver:** die Fernsicht von oben.

**Utility**
- **Consumable-Optimizer:** Essen und Trinken enden im richtigen Tick, ohne auf den Server zu
  warten.
- **No Tool Break:** sperrt das Abbauen, bevor ein Diamant- oder Netherit-Werkzeug mit
  Reparatur zerbricht.
- **Zoom:** verengt das Sichtfeld, solange die Zoom-Taste gedrückt ist.
- **Sprachchat:** kompatibel zu Simple Voice Chat. Du hörst Spieler in der Nähe und sprichst
  mit der Sprechtaste.
- **Discord-Status:** zeigt Version, Server und Spielzeit in Discord an.
- **Input-Log:** schreibt Tasten, Maustasten und das Mausrad mit Zeitstempel in eine Datei.
- **Nachhaltigkeit:** spart Strom. Im Hintergrund zeichnet der Client keine Bilder, und hinter
  dem Pausemenü steht die Welt still.

**Debug**
- **Ghostblock-Indikator:** Blöcke, die der Server noch nicht bestätigt hat (gesetzt,
  abgebaut, Anker), werden nach einer einstellbaren Toleranz halbdurchsichtig.
- **Botcheck-Pass:** lädt beim Betreten erst die Welt und läuft dann los, wie Vanilla. Das ist
  langsamer, besteht aber Botchecks.
- **Barrieren anzeigen:** macht Barrieren, Lichtblöcke und Strukturleere sichtbar.
- **Sound-Debug:** zwei Balken für die Lautstärke des linken und rechten Kanals.

### Bedienung

- Die Oberfläche gibt es auf Deutsch, Englisch und Latein.
- Mehrere Einstellungs-Configs, im Launcher wählbar.
- Ein Spielmodus-Rad und frei belegbare Tasten, auch doppelt belegt.
- Die Menüs sind von [osu!lazer](https://github.com/ppy/osu) inspiriert: Knöpfe leuchten beim
  Hovern auf, Markierungen leuchten beim Aufziehen, der Cursor gleitet, und UI-Klänge begleiten
  die Eingaben. osu! war die Vorlage für Zeiten und Verhalten. Code daraus enthält Coral nicht.

Version 1.0 ist das erste stabile Release. Vieles funktioniert wie in Vanilla, manches fehlt
noch oder sieht anders aus. Gebaut und getestet wird der Client gegen 1.21.11. Andere Versionen
werden nicht unterstützt.

### Selbst bauen

Du brauchst eine aktuelle Rust-Toolchain (stable). Unter Linux brauchst du außerdem die
ALSA-Entwicklungsdateien (`libasound2-dev` o. ä.).

```sh
cargo build --release -p app -p launcher
```

Danach liegen `target/release/coral-client` und `target/release/coral-client-launcher` bereit. Der
Launcher startet den Client, der im selben Ordner liegt.

| Crate | Aufgabe |
|---|---|
| `protocol` | Pakete des Minecraft-Protokolls lesen und schreiben |
| `net` | Verbindung zum Server, Login, Verschlüsselung |
| `auth` | Microsoft-Anmeldung |
| `world` | Weltzustand aus den Serverpaketen: Chunks, Entities, Items |
| `assets` | Laden aus der Minecraft-Installation und aus Resourcepacks |
| `render` | Zeichnen mit wgpu: Gelände, Fernsicht, Himmel, Shader-Pfad |
| `chunk_saver` | Gespeicherte Chunks und ihre Fernsicht |
| `audio` | Tonausgabe mit niedriger Latenz |
| `voice` | Sprachchat |
| `app` | Fenster, Eingabe, Oberfläche, Spiellogik, hält alles zusammen |
| `launcher` | Installation wählen, Updates, Configs, Starten |

## Lizenz und Beiträge

**Alle Rechte vorbehalten.** Der Code ist öffentlich, damit du ihn lesen, daraus lernen und
beim Verbessern helfen kannst. Frei weiterveröffentlichen darfst du ihn deshalb nicht.

- **Erlaubt:** dieses Repository auf GitHub forken, in deinem Fork Änderungen machen und Pull
  Requests stellen. Die Releases herunterladen und benutzen ist natürlich auch erlaubt.
- **Nicht erlaubt:** den Code irgendwo anders veröffentlichen als in einem GitHub-Fork dieses
  Repositorys, zum Beispiel als neues Repository, als Download oder als Teil eines anderen
  Clients. Das gilt auch, wenn du die Quelle nennst.

**Ausnahme Launcher:** Alles in `crates/launcher/` ist unter [CC0 1.0](crates/launcher/LICENSE)
gemeinfrei. Den fertigen Launcher (`coral-client-launcher-*` aus den Releases) darfst du frei
teilen, hochladen und weitergeben, einschließlich der Teile des `assets`-Crates, mit denen er
gebaut ist.

**Pull Requests:** Mit einem Pull Request erlaubst du dem Autor, deinen Beitrag in diesem
Projekt unter jeder Lizenz zu verwenden, auch unter einer späteren Open-Source-Lizenz.

### Dateien von Dritten

Diese Dateien fallen nicht unter das oben Gesagte und behalten ihre eigene Lizenz:

| Dateien | Herkunft | Lizenz |
|---|---|---|
| `assets/fonts/` | DM Sans, Lexend, Lexend Exa, Noto Sans, Outfit | SIL Open Font License 1.1 (Lizenzdateien neben den Schriften) |
| `assets/sounds/kenney-*` | [Kenney](https://kenney.nl): Interface Sounds, UI Audio | CC0 1.0 |
| `assets/sounds/custom/` | aus Kenney-Klängen und Synthese gebaut | CC0 1.0 |
| `assets/sounds/totemingpromax/click.wav` | [Kenney](https://kenney.nl): Interface Sounds | CC0 1.0 |
| `assets/sounds/totemingpromax/glass_break_01.wav` | [rubberduck: 75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) | CC0 1.0 |

## Rechtliches

- Das ist **kein offizielles Minecraft-Produkt**. Es ist nicht von Mojang oder Microsoft
  genehmigt und steht in keiner Verbindung zu ihnen. „Minecraft“ ist eine Marke von Mojang AB.
- Coral Client wird von DerAntonia entwickelt und veröffentlicht, die Verantwortung liegt allein
  dort, nicht bei Mojang oder Microsoft.
- Kontakt: am besten auf Discord: `derantonia`, dort antworte ich sehr wahrscheinlich. Die
  E-Mail-Adresse hubertjohnson1231236@gmail.com gibt es auch, dort schaue ich aber kaum rein.
- Das Projekt enthält keine Texturen, Klänge, Modelle oder Code von Mojang. Alles
  Spielmaterial kommt zur Laufzeit aus deiner eigenen Installation. Enthalten sind aber
  Tabellen mit Spieldaten, die aus dem Spiel ausgelesen sind (Block-Zustände, Kollisionsformen,
  Abbauwerte), wie es auch Projekte wie PrismarineJS tun.
- Der Client umgeht keine Anticheats und verschafft keine unfairen Vorteile. Er macht, was
  Vanilla macht, nur mit weniger Verzögerung.
