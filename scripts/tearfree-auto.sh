#!/usr/bin/env bash
# Schaltet TearFree (AMD-spezifische xrandr-Eigenschaft) automatisch je nach fokussiertem Fenster um:
# Browser -> an (reissfrei, Latenz spielt dort keine Rolle), coral-client/Spiele
# -> auto/aus (niedrigste Latenz, Tearing bewusst in Kauf genommen).
#
# Das kann NICHT im Coral Client selbst passieren: TearFree ist eine globale
# Anzeigeeinstellung, und dieses Skript muss auch reagieren, wenn Coral Client
# gar nicht laeuft (z.B. beim Wechsel zu einem Browser-Fenster). Es laeuft
# deshalb als eigenstaendiges, vom Nutzer gestartetes Werkzeug -- genau wie
# latency-tuning.sh, nur ereignisgetrieben statt start/stop.
#
# Ereignisgetrieben statt gepollt: xprop -spy blockiert und meldet nur echte
# Aenderungen des aktiven Fensters, kein "while true; sleep"-Busy-Loop.
#
# Usage: tearfree-auto.sh (im Vordergrund laufen lassen, Strg+C zum Beenden)
set -euo pipefail

LOCK_NAME="tearfree-auto.sh"

# Browser-Fensterklassen (WM_CLASS, klein geschrieben) -- TearFree an, da hier
# Latenz irrelevant ist und Tearing beim Scrollen/Video sofort auffaellt.
BROWSER_PATTERNS=("firefox" "navigator" "chrome" "chromium" "brave" "librewolf")

# Anwendungen, fuer die TearFree explizit aus sein soll (niedrigste Latenz).
# Coral Client setzt seine WM_CLASS bewusst stabil (s. main.rs, with_name), daher
# hier exakt erkennbar statt zu raten.
LOW_LATENCY_PATTERNS=("coral-client")

if [[ "$(pgrep -fc "$LOCK_NAME")" -gt 1 ]]; then
    echo "tearfree-auto.sh laeuft bereits (s. pgrep -fa $LOCK_NAME) -- nicht doppelt starten." >&2
    exit 1
fi

for tool in xprop xdotool xrandr; do
    command -v "$tool" >/dev/null || { echo "benoetigt '$tool', ist aber nicht installiert." >&2; exit 1; }
done

OUTPUT="$(xrandr | awk '/ connected/ {print $1; exit}')"
if [[ -z "$OUTPUT" ]]; then
    echo "kein verbundener Ausgang gefunden." >&2
    exit 1
fi

echo "TearFree-Automatik fuer Ausgang '$OUTPUT'. Strg+C zum Beenden."

last_value=""
set_tearfree() {
    local value="$1"
    if [[ "$value" != "$last_value" ]]; then
        xrandr --output "$OUTPUT" --set TearFree "$value" && last_value="$value"
        echo "[tearfree-auto] $value ($2)"
    fi
}

# Beim Beenden (Strg+C, SIGTERM) sauber auf den Standardwert zurueck -- das Skript
# darf den Desktop nicht in einem selbst gesetzten Zustand hinterlassen.
cleanup() {
    xrandr --output "$OUTPUT" --set TearFree auto 2>/dev/null || true
    echo "TearFree zurueckgesetzt auf 'auto'."
}
trap cleanup EXIT INT TERM

matches_any() {
    local haystack="$1"
    shift
    local pattern
    for pattern in "$@"; do
        [[ "$haystack" == *"$pattern"* ]] && return 0
    done
    return 1
}

# xprop -spy blockiert und gibt eine Zeile aus, sobald sich das aktive Fenster
# aendert -- kein Poll-Intervall, keine Verzoegerung, keine Busy-Loop-Schleife.
xprop -root -spy _NET_ACTIVE_WINDOW 2>/dev/null | while read -r _; do
    win_id="$(xdotool getactivewindow 2>/dev/null)" || continue
    class="$(xprop -id "$win_id" WM_CLASS 2>/dev/null \
        | sed -n 's/.*= "[^"]*", "\([^"]*\)"/\1/p' \
        | tr '[:upper:]' '[:lower:]')"
    [[ -z "$class" ]] && continue

    if matches_any "$class" "${LOW_LATENCY_PATTERNS[@]}"; then
        set_tearfree auto "$class"
    elif matches_any "$class" "${BROWSER_PATTERNS[@]}"; then
        set_tearfree on "$class"
    fi
    # Alles andere (Editor, Terminal, ...): nicht anfassen -- normale Desktop-
    # Nutzung stresst den Praesentationspfad nicht genug, um TearFree zu brauchen,
    # und staendiges Umschalten fuer jedes Fenster waere nur unnoetiges Flackern.
done
