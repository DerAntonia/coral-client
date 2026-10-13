#!/usr/bin/env bash
# Startet den Coral Client mit Latenz-Tuning (CPU-Governor "performance" +
# USB-Autosuspend der Maus deaktiviert, siehe latency-tuning.sh) und setzt
# beim Beenden (auch bei Strg+C/Absturz) automatisch die vorherigen Werte
# wieder her, damit Luefter/Power-Management im Alltag nicht dauerhaft
# hochgedreht bleiben.
#
# CORAL_PERFORMANCE=1 schaltet zusaetzlich die erhoehte Prozess-Prioritaet
# (nice -10) im Client frei -- ohne diese Variable laeuft er auf Standard-Prioritaet.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

trap './scripts/latency-tuning.sh stop' EXIT

./scripts/latency-tuning.sh start
# --release ist hier Pflicht, nicht Kosmetik: das Chunk-Meshing ist eine reine
# Rechenschleife und im Debug-Build gemessene 19x langsamer (32ms statt 2ms pro
# Chunk-Spalte). Ein Debug-Build wuerde beim Join auf ~15 FPS einbrechen -- damit
# waere jede Latenz-Messung an diesem Skript wertlos.
# Ausgabe zusaetzlich mitschreiben: Panics in tokio-Worker-Threads toeten nur den
# jeweiligen Task, nicht den Prozess -- die App laeuft dann scheinbar normal weiter und
# die einzige Spur steht hier drin.
# Den Mitschnitt des VORIGEN Laufs aufheben, bevor tee ihn ueberschreibt. Grund: beim
# Nachgehen eines Grim-Flags braucht man client.log und das Server-Log DESSELBEN Laufs --
# und ein zweiter Start hat die Spur schon mehr als einmal geloescht, bevor sie gelesen war.
[ -f client.log ] && mv client.log client.log.1
CORAL_PERFORMANCE=1 RUST_BACKTRACE=1 cargo run --release -p app 2>&1 | tee client.log
