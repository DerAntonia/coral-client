#!/usr/bin/env bash
# Verwaltet CPU-Governor + USB-Autosuspend (fuer die Maus) gemeinsam fuer
# minimale Input-Latenz. "start" merkt sich die vorherigen Werte in einer
# State-Datei, "stop" stellt sie wieder her (z.B. um im Alltag wieder leise
# Luefter/normales Power-Management zu haben).
#
# Usage: latency-tuning.sh {start|stop|status}
set -euo pipefail

STATE_FILE="/tmp/coral-client-latency-tuning.state"
MOUSE_VENDOR="1e7d"
MOUSE_PRODUCT="2c88"

governor_files() {
    local f
    for f in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
        echo "$f"
    done
}

current_governor() {
    local first
    first="$(governor_files | head -n1)"
    cat "$first"
}

find_mouse_sysfs_path() {
    local dev
    for dev in /sys/bus/usb/devices/*/; do
        if [[ -f "${dev}idVendor" && -f "${dev}idProduct" ]]; then
            if [[ "$(cat "${dev}idVendor")" == "$MOUSE_VENDOR" && "$(cat "${dev}idProduct")" == "$MOUSE_PRODUCT" ]]; then
                echo "${dev%/}"
                return 0
            fi
        fi
    done
    return 1
}

cmd_status() {
    echo "Governor: $(current_governor)"
    local mouse_path
    if mouse_path="$(find_mouse_sysfs_path)"; then
        echo "Maus (${mouse_path}): power/control = $(cat "${mouse_path}/power/control")"
    else
        echo "Maus nicht gefunden (nicht angeschlossen?)"
    fi
    if [[ -f "$STATE_FILE" ]]; then
        echo "Getunt seit: ja (State-Datei vorhanden, 'stop' zum Zuruecksetzen)"
    else
        echo "Getunt seit: nein"
    fi
}

cmd_start() {
    if [[ -f "$STATE_FILE" ]]; then
        echo "[latency-tuning] laeuft bereits (State-Datei vorhanden) -- erst 'stop' ausfuehren"
        exit 1
    fi

    local governor_now mouse_path mouse_control_now
    governor_now="$(current_governor)"
    mouse_path="$(find_mouse_sysfs_path || true)"

    {
        echo "PREV_GOVERNOR=${governor_now}"
        if [[ -n "${mouse_path:-}" ]]; then
            mouse_control_now="$(cat "${mouse_path}/power/control")"
            echo "PREV_MOUSE_CONTROL=${mouse_control_now}"
            echo "MOUSE_PATH=${mouse_path}"
        fi
    } > "$STATE_FILE"

    echo "[latency-tuning] Governor: ${governor_now} -> performance"
    echo performance | sudo tee $(governor_files) > /dev/null

    if [[ -n "${mouse_path:-}" ]]; then
        echo "[latency-tuning] USB-Autosuspend Maus (${mouse_control_now}) -> on"
        echo on | sudo tee "${mouse_path}/power/control" > /dev/null
    else
        echo "[latency-tuning] Maus nicht gefunden, USB-Autosuspend uebersprungen"
    fi
}

cmd_stop() {
    if [[ ! -f "$STATE_FILE" ]]; then
        echo "[latency-tuning] kein gespeicherter Zustand gefunden, nichts zu tun"
        return
    fi

    # shellcheck disable=SC1090
    source "$STATE_FILE"

    echo "[latency-tuning] Governor -> ${PREV_GOVERNOR}"
    echo "${PREV_GOVERNOR}" | sudo tee $(governor_files) > /dev/null

    if [[ -n "${MOUSE_PATH:-}" && -n "${PREV_MOUSE_CONTROL:-}" ]]; then
        echo "[latency-tuning] USB-Autosuspend Maus -> ${PREV_MOUSE_CONTROL}"
        echo "${PREV_MOUSE_CONTROL}" | sudo tee "${MOUSE_PATH}/power/control" > /dev/null
    fi

    rm -f "$STATE_FILE"
}

case "${1:-}" in
    start) cmd_start ;;
    stop) cmd_stop ;;
    status) cmd_status ;;
    *)
        echo "Usage: $0 {start|stop|status}"
        exit 1
        ;;
esac
