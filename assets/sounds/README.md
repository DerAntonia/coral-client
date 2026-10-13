# Klänge der Oberfläche

## Herkunft und Lizenz

Zwei Quellen, zwei unterschiedliche Lizenzen -- nicht verwechseln.

**Kenney** (kenney.nl), **Creative Commons CC0 1.0** — gemeinfrei, Nutzung
auch kommerziell erlaubt, Namensnennung **nicht erforderlich**, Weitergabe
und Bearbeitung ausdrücklich gestattet. Lizenztext: `LICENSE-kenney.txt`.

| Ordner | Paket | Dateien | Quelle |
| --- | --- | --- | --- |
| `kenney-interface/` | Interface Sounds | 100 | https://kenney.nl/assets/interface-sounds |
| `kenney-ui/` | UI Audio | 51 | https://kenney.nl/assets/ui-audio |

Aus dem UI-Audio-Paket ist `preview.ogg` **nicht** übernommen: das ist eine
14-Sekunden-Vorführung aller Klänge hintereinander, kein einzelner Klang.

## Warum WAV, obwohl Kenney OGG ausliefert

Die Originalpakete enthalten OGG-Vorbis. Die Dateien hier sind daraus einmalig
umgewandelt:

```sh
ffmpeg -i <datei>.ogg -ac 1 -ar 48000 -sample_fmt s16 <datei>.wav
```

Damit braucht der Client **keinen Ogg-Dekoder und keine Audio-Abhängigkeit** —
eine WAV-Datei ist ein Kopf und danach rohe Abtastwerte, das liest
`audio::wav` in ein paar Dutzend Zeilen. Der Preis ist Plattenplatz: unkomprimiert
sind es 4,4 MB statt 1,2 MB. Für einen Satz UI-Klänge ist das der bessere Tausch.

**Mono**, weil die Stereo-Lage der Mixer selbst vergibt (`audio::pan_gains`) — eine
in der Datei mitgelieferte Lage würde damit nur in Konflikt geraten.

**48 kHz**, weil der Mixer das anstrebt (`PREFERRED_SAMPLE_RATE`). Läuft das Gerät
mit einer anderen Rate, rechnet `Wav::sample_at` linear um; bei 48 kHz ist das ein
Nulldurchgang ohne Rechenaufwand.

## Wie die Klänge ins Binary kommen

`crates/audio/build.rs` durchläuft diesen Ordner und erzeugt eine Tabelle mit
`include_bytes!`. Eingebettet statt zur Laufzeit geladen, damit der Client aus jedem
Arbeitsverzeichnis startet.

Neue Dateien hier ablegen genügt — die Tabelle entsteht beim nächsten Build neu.

## Welcher Klang wo verwendet wird

Steht in `UiSound::sample()` in `crates/app/src/ui_sound.rs` -- aktuell nur
**ein** Eintrag (`Type`, s. `custom/click_002_tief10.wav`). Eine erste Fassung
hatte alle zehn Klänge nach Dateinamen zugeordnet, nicht nach Gehör; das kam
so schlecht an, dass sie bis auf den einen durchgehörten und bestätigten
Klang wieder zurückgenommen wurde.

Die übrigen 150 Aufnahmen bleiben trotzdem hier: `scripts/klaenge-anhoeren.sh`
braucht sie zum Durchhören, wenn als nächstes ein weiterer Klang ausgesucht
wird. Dann in `sample()` eintragen -- ein Test stellt sicher, dass jeder
eingetragene Name wirklich existiert, ohne ihn fiele ein Tippfehler nicht auf,
weil der Client dann stillschweigend auf den synthetischen Klang zurückfällt.
