#!/usr/bin/env bash
# Die mitgelieferten UI-Klaenge der Reihe nach anhoeren und den Namen dazu sehen.
#
# Der Grund fuer dieses Skript: die Zuordnung in crates/app/src/ui_sound.rs wurde nach
# DATEINAMEN geraten, nicht nach Gehoer. Wer hoeren kann, soll auswaehlen -- danach ist es je
# eine Zeile Aenderung.
#
#   ./scripts/klaenge-anhoeren.sh                       # alles, alphabetisch
#   ./scripts/klaenge-anhoeren.sh click                 # nur Namen, die "click" enthalten
#   ./scripts/klaenge-anhoeren.sh --pause 0.8           # mit 0,8 s Pause dazwischen
#   ./scripts/klaenge-anhoeren.sh click --halbtoene -3  # 3 Halbtoene tiefer
#   ./scripts/klaenge-anhoeren.sh --halbtoene 7         # alles, 7 Halbtoene hoeher
#
# Absichtlich Flags statt reiner Positionsargumente: bei "muster pause halbtoene" laesst sich
# der letzte Parameter nicht setzen, ohne die mittleren mit anzugeben. Ein Versuch, das mit
# einem platzhalterhaften "*" zu umgehen, expandiert in der Shell VOR dem Aufruf zu allen
# Dateien im aktuellen Ordner (Cargo.toml, Cargo.lock, ...) und verschiebt dann heimlich alle
# nachfolgenden Argumente -- genau das ist einmal passiert.
#
# Waehrend es laeuft: Strg+C bricht ab. Der zuletzt genannte Name ist der, den man dann in
# UiSound::sample() eintraegt.

set -u
cd "$(dirname "$0")/.." || exit 1

muster=""
pause="0.45"
halbtoene="0"

while [ $# -gt 0 ]; do
    case "$1" in
        --pause|-p)
            pause="${2:?--pause braucht einen Wert}"
            shift 2
            ;;
        --pause=*)
            pause="${1#*=}"
            shift
            ;;
        --halbtoene|--halbton|-n)
            # Bewusst UNGEPRUEFT auf ein "-" am Anfang: -10 Halbtoene ist ein gueltiger,
            # haeufiger Wert (tiefer), kein neues Flag.
            halbtoene="${2:?--halbtoene braucht einen Wert}"
            shift 2
            ;;
        --halbtoene=*|--halbton=*)
            halbtoene="${1#*=}"
            shift
            ;;
        --hilfe|-h|--help)
            sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        --)
            shift
            break
            ;;
        -*)
            echo "Unbekannte Option: $1 (--hilfe fuer eine Uebersicht)" >&2
            exit 1
            ;;
        *)
            if [ -n "$muster" ]; then
                echo "Nur EIN Suchmuster gleichzeitig -- \"$muster\" ist schon gesetzt, \"$1\" kaeme dazu." >&2
                exit 1
            fi
            muster="$1"
            shift
            ;;
    esac
done

if command -v paplay >/dev/null 2>&1; then
    abspielen() { paplay "$1" 2>/dev/null </dev/null; }
elif command -v pw-play >/dev/null 2>&1; then
    abspielen() { pw-play "$1" 2>/dev/null </dev/null; }
elif command -v aplay >/dev/null 2>&1; then
    abspielen() { aplay -q "$1" 2>/dev/null </dev/null; }
elif command -v ffplay >/dev/null 2>&1; then
    abspielen() { ffplay -nodisp -autoexit -nostdin -loglevel quiet "$1" </dev/null; }
else
    echo "Kein Abspielprogramm gefunden (paplay, pw-play, aplay oder ffplay)." >&2
    exit 1
fi

# Halbtonverschiebung braucht ffmpeg -- Tonhoehe UND Laenge werden dabei bewusst beide
# verschoben (asetrate aendert beides gleichzeitig), nicht nur die Tonhoehe bei fester Laenge:
# genau so haette eine andere Aufnahmegeschwindigkeit auch geklungen, und ein Klick, der drei
# Halbtoene tiefer UND laenger ist, ist die ehrliche Antwort auf "wie klingt das tiefer".
tmpdir=""
verschieben() {
    if [ ! -x "$(command -v ffmpeg 2>/dev/null)" ]; then
        echo "Halbtonverschiebung braucht ffmpeg, das ist hier nicht installiert." >&2
        exit 1
    fi
    tmpdir=$(mktemp -d)
    trap 'rm -rf "$tmpdir"' EXIT
    # Faktor 2^(n/12) je Halbton -- die Tonleiter ist logarithmisch, nicht linear.
    # LC_NUMERIC=C erzwingt einen Punkt als Dezimaltrennzeichen: bei deutschem Locale liefert
    # awk sonst ein Komma ("0,84"), und ffmpeg liest das als zwei Filter-Namen statt einer Zahl.
    faktor=$(LC_NUMERIC=C awk -v n="$halbtoene" 'BEGIN { print 2 ^ (n / 12) }')
}

anzeige_halbtoene=""
if [ "$halbtoene" != "0" ]; then
    verschieben
    anzeige_halbtoene=" (${halbtoene} Halbtoene)"
fi

dateien=$(find assets/sounds -name '*.wav' | sort)
if [ -n "$muster" ]; then
    dateien=$(printf '%s\n' "$dateien" | grep -i -- "$muster")
fi

anzahl=$(printf '%s\n' "$dateien" | grep -c . )
if [ "$anzahl" -eq 0 ]; then
    echo "Nichts gefunden fuer \"$muster\"."
    echo "Vorhandene Kategorien:"
    find assets/sounds -name '*.wav' -printf '%f\n' | sed 's/[0-9]*\.wav$//' | sort -u | tr '\n' ' '
    echo
    exit 1
fi

echo "$anzahl Klaenge${anzeige_halbtoene}. Strg+C bricht ab."
echo
zaehler=0
printf '%s\n' "$dateien" | while read -r f; do
    # Name genau so, wie er in UiSound::sample() gehoert: ohne "assets/sounds/" und ohne ".wav"
    name="${f#assets/sounds/}"
    printf '  %s\n' "${name%.wav}"
    if [ -n "$tmpdir" ]; then
        zaehler=$((zaehler + 1))
        verschoben="$tmpdir/$zaehler.wav"
        # -nostdin UND </dev/null zusammen: ffmpeg fragt sonst still an derselben Standard-
        # eingabe mit, an der auch die "read"-Schleife haengt (dieselbe Pipe, aus find/sort),
        # und klaut ihr dabei Zeichen -- das war der Grund, warum jede zweite Zeile einen
        # abgeschnittenen Dateinamen ("ssets/..." statt "assets/...") ausgab.
        ffmpeg -nostdin -loglevel error -y -i "$f" -af "asetrate=48000*${faktor},aresample=48000" "$verschoben" </dev/null
        abspielen "$verschoben"
    else
        abspielen "$f"
    fi
    sleep "$pause"
done
