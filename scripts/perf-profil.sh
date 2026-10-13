#!/usr/bin/env bash
# CPU-Profil des Coral Client mit `perf`. Startet den Client ganz normal (Fenster geht auf,
# man spielt eine Weile) und schreibt beim Beenden eine Auswertung nach perf-report.txt.
#
# Bewusst OHNE latency-tuning.sh: das braucht sudo und veraendert CPU-Governor und
# USB-Autosuspend. Ein Profil soll den Zustand messen, in dem es klemmt, und nicht den
# hochgetunten Sonderfall -- wer das Tuning mitmessen will, startet vorher
# ./scripts/latency-tuning.sh start und danach ./scripts/latency-tuning.sh stop.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

BIN=target/release/coral-client
DAUER_HINWEIS="Zum Beenden im Client normal rausgehen oder hier Strg+C -- perf schreibt die Daten erst danach."

if [ ! -x "$BIN" ]; then
    echo "Kein Release-Build da. Erst bauen: cargo build --release -p app" >&2
    exit 1
fi

# perf_event_paranoid >= 3 verbietet auch das Profilieren eigener Prozesse. Ubuntu setzt
# hier 4. Den Client stattdessen unter sudo laufen zu lassen ist keine Loesung: er wuerde
# als root auf X11/GPU und auf eine andere Konfiguration zugreifen.
PARANOID=$(cat /proc/sys/kernel/perf_event_paranoid)
if [ "$PARANOID" -ge 3 ]; then
    echo "perf_event_paranoid steht auf $PARANOID -- perf darf so nichts messen." >&2
    echo "Einmalig (bis zum naechsten Neustart) freischalten mit:" >&2
    echo "    sudo sysctl kernel.perf_event_paranoid=1" >&2
    exit 1
fi

# 499 Hz statt der glatten 500: eine krumme Frequenz kann sich nicht auf die 20-Hz-Ticks
# oder die Bildrate einrasten, sonst misst man systematisch immer dieselbe Stelle im Frame.
# --call-graph fp funktioniert, weil der Release-Build in Cargo.toml Frame-Pointer erzwingt.
echo "$DAUER_HINWEIS"
perf record -F 499 -g --call-graph fp -o perf.data -- "$BIN" 2>&1 | tee perf-client.log || true

echo
echo "Auswertung laeuft ..."
{
    echo "=== Flach: wo die CPU-Zeit wirklich verbrannt wird (Selbstzeit) ==="
    perf report -i perf.data --stdio --no-children -g none --percent-limit 0.3 --sort dso,symbol
    echo
    echo "=== Aufrufbaeume: welcher Pfad die Zeit verursacht (Gesamtzeit) ==="
    perf report -i perf.data --stdio --children -g graph,0.5,caller --percent-limit 1
} > perf-report.txt 2>&1

echo "Fertig: perf-report.txt (Rohdaten in perf.data, Client-Ausgabe in perf-client.log)"
echo "Interaktiv durchklicken: perf report -i perf.data"
