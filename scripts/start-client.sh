#!/bin/sh
# Startskript hinter dem Schreibtisch-Symbol (coral-client.desktop).
#
# Eigenes Skript statt einer Kommandozeile direkt im .desktop: die Desktop-Entry-Spezifikation
# verbietet Shell-Sonderzeichen in Exec= (>, &, Anfuehrungszeichen), die Ausgabeumleitung geht
# dort also nicht.
#
# WICHTIG -- ECHTER FEHLER GEWESEN (2026-08-22): dieses Skript hat frueher NUR gestartet, nie
# gebaut. Nach mehreren Physik-Korrekturen wurde per Symbol weiter ein fuenf Tage altes Binary
# gestartet, und die Grim-Flags kamen bitgenau identisch zurueck -- es sah aus, als wirke kein
# einziger Fix. Deshalb prueft das Skript jetzt, ob das Binary aelter ist als die Quellen, und
# baut in einem SICHTBAREN Terminalfenster nach. Sichtbar, weil ein stiller Bau von ein paar
# Minuten beim Doppelklick wie ein Haenger aussieht -- und weil ein Bau Rechenzeit ist, die man
# laufen sehen soll.
set -e
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bin="$repo/target/release/coral-client"

veraltet=1
if [ -x "$bin" ]; then
    # Irgendeine Quelldatei neuer als das Binary?
    neuer=$(find "$repo/crates" "$repo/Cargo.toml" "$repo/Cargo.lock" -newer "$bin" -print -quit 2>/dev/null || true)
    [ -z "$neuer" ] && veraltet=0
fi

if [ "$veraltet" = 1 ]; then
    # In einem Terminal bauen und danach direkt starten; das Fenster bleibt bei einem
    # Baufehler offen, sonst sieht man die Fehlermeldung nie.
    befehl="cd '$repo' && cargo build --release && exec '$bin' 2>&1 | tee -a '$repo/start.log'"
    if command -v gnome-terminal > /dev/null 2>&1; then
        exec gnome-terminal --title="Coral Client bauen" -- sh -c "$befehl || { echo; echo 'Bau fehlgeschlagen -- Fenster bleibt offen.'; read -r _; }"
    elif command -v x-terminal-emulator > /dev/null 2>&1; then
        exec x-terminal-emulator -e sh -c "$befehl || { echo; echo 'Bau fehlgeschlagen.'; read -r _; }"
    elif command -v zenity > /dev/null 2>&1; then
        zenity --error --no-wrap --title="Coral Client" --text="Das gebaute Binary ist aelter als der Quellcode.\n\nErst bauen:\ncd \"$repo\" && cargo build --release"
        exit 1
    else
        echo "Binary veraltet -- erst bauen: cd \"$repo\" && cargo build --release" >&2
        exit 1
    fi
fi

cd "$repo"
exec "$bin" >> "$repo/start.log" 2>&1
