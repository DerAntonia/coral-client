# Abgeleitete Klänge

Dieser Ordner enthält **bearbeitete** Kenney-Aufnahmen -- Original in
`kenney-interface/` oder `kenney-ui/`, hier verändert nach Gehör des Nutzers.
CC0 erlaubt das ohne Auflagen (s. `../LICENSE-kenney.txt`); dokumentiert wird es
trotzdem, damit die Herkunft nachvollziehbar bleibt.

Bearbeitet wird hier, weil der Mixer selbst KEINE Tonhöhenverschiebung kann --
`scripts/klaenge-anhoeren.sh` verschiebt beim Durchhören per ffmpeg, aber das
Spiel spielt nur unveränderte Dateien ab (s. `audio::wav`). Ein per Skript
gehörter, gepitchter Klang muss deshalb einmalig genauso vorgerechnet und als
eigene Datei abgelegt werden.

## click_002_tief10.wav

**Aktuell NICHT verdrahtet** -- `UiSound::Type` spielt zur Zeit
`kenney-ui/mouseclick1` unverändert, während der Nutzer mehrere Kandidaten
durchhört ("bei sounddesign muss ich mal alle reinhören"). Die Datei bleibt
liegen, falls dieser Kandidat sich am Ende doch durchsetzt -- sonst müsste die
Verschiebung ein zweites Mal gerechnet werden.

Quelle: `kenney-interface/click_002.wav`, 10 Halbtöne tiefer. Vom Nutzer über

```sh
./scripts/klaenge-anhoeren.sh kenney-interface/click_002 --halbtoene -10
```

durchgehört und für `UiSound::Type` (Tastaturtippen) bestätigt.

**Exakt** dieselbe Filterkette wie im Skript, keine weitere Bearbeitung:

```sh
faktor=$(LC_NUMERIC=C awk -v n="-10" 'BEGIN { print 2 ^ (n / 12) }')
ffmpeg -i assets/sounds/kenney-interface/click_002.wav \
  -af "asetrate=48000*${faktor},aresample=48000" \
  assets/sounds/custom/click_002_tief10.wav
```

Ergebnis: 995 Frames, 48 kHz, mono, 20,7 ms, Spitze 0,909. Die Rohdatei ist mit
12 ms bereits einer der kürzesten Klänge im ganzen Kenney-Satz -- die
Verschiebung dehnt sie auf etwas Hörbares. Kürzer als die allgemeine
25-ms-Richtlinie für Aufnahmen (`UiSound::min_sample_ms`), aber das ist eine
Vermutung über BLIND (nach Dateinamen) gewählte Klänge, keine Regel, die über
einen tatsächlich gehörten und bestätigten Klang entscheidet.

Eine erste Fassung dieser Datei stammte fälschlich aus `kenney-ui/click2.wav`
(Verwechslung der beiden Pakete, nie vom Nutzer gehört) und war zusätzlich bei
55 ms gekappt. Beides war eigenmächtig und wurde zurückgenommen.

## scroll_003_klick.wav

Ein einzelner Klick aus `kenney-interface/scroll_003.wav`. Das Original ist eine Folge von
20 Klicks im Abstand von genau 50 ms, alle 20 sind identisch (Korrelation 1,000). Geschnitten
für den Sound-Baukasten, damit sich der Klick einzeln schichten lässt.

- Samples 0 bis 719 des Originals (15 ms). Nach etwa 12 ms ist der Klick abgeklungen.
- Sample 0 auf 0 gesetzt: Es ist ein Rest des vorigen Klicks aus der OGG-Umwandlung, der Klick
  selbst beginnt bei Sample 1.
- Gleichspannungsversatz von −0,00615 (−44 dB) abgezogen. Er stand nach jedem Klick bis zum
  nächsten und ist ebenfalls ein Überbleibsel der Umwandlung.
- Die letzten 3 ms linear ausgeblendet.

Ergebnis: 720 Frames, 48 kHz, mono, 15 ms, Spitze 0,913.

## ui/ -- die UI-Klänge des Clients

Je Platz eine Datei (`ui_hover.wav`, `ui_click.wav`, …), gebaut vom Nutzer nach Gehör im
Sound-Baukasten und von dort exportiert: genau so gerendert, wie sie dort klingen (16 Bit,
48 kHz, mono), ohne die Tonhöhen-Streuung, die der Client bei jedem Abspielen selbst würfelt
(s. `UiSound::spread_semitones`).

Ausgangsmaterial:

- Kenney-Aufnahmen aus `kenney-interface/` (CC0), dort gepitcht, gestreckt und geschichtet.
- Eigene Synthese des Baukastens (Sinus mit Obertönen und FM, gefiltertes Rauschen), darunter
  zwei Nachbauten nach Messung von osu!lazer-Klängen. Das sind keine Kopien, sondern Parameter,
  die per Suche auf ein ähnliches Spektrum angepasst wurden. Keine Datei aus osu! steckt hier
  drin.

Danach je Platz Klangformung (Tiefpass, Absenkung um 3,2 kHz, weicher Anschlag) und ein
Limiter. Was genau in jedem Platz steckt, mit allen Parametern, steht in `zuordnung.txt`;
`zuordnung.json` enthält Streuung, Mindestabstand und den Slider-Anstieg.

Gesperrt ist im Baukasten leer und bleibt im Client stumm.
