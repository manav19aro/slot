#!/bin/sh
# One explicit download connection. BaseOS owns the driver; Slot owns association.
set -u
RUN="${SLOT_RUN:-/run}/slot-download"
CTRL="$RUN/wpa"
NET="${AGS_NET_SYS:-/sys/class/net}"
cleanup() {
    wpa_cli -p "$CTRL" -i wlan0 terminate >/dev/null 2>&1 || true
    if [ -f "$RUN/dhcp.pid" ]; then
        read -r pid < "$RUN/dhcp.pid"
        case "$pid" in ''|*[!0-9]*) ;; *) kill "$pid" 2>/dev/null || true ;; esac
    fi
    ip route del default dev wlan0 >/dev/null 2>&1 || true
    ip addr flush dev wlan0 >/dev/null 2>&1 || true
    ip link set wlan0 down >/dev/null 2>&1 || true
    rm -f "$RUN/dhcp.pid" "$RUN/network.conf" "$CTRL/wlan0"
    rmdir "$CTRL" "$RUN" 2>/dev/null || true
}
case "${1:-}" in
up)
    trap 'cleanup; exit 1' HUP INT TERM
    i=0
    while [ ! -d "$NET/wlan0" ] && [ "$i" -lt 100 ]; do sleep 0.25; i=$((i+1)); done
    [ -d "$NET/wlan0" ] || exit 1
    mkdir -p "$CTRL"
    rfkill unblock wifi 2>/dev/null || true
    ip link set wlan0 up || exit 1
    wpa_supplicant -B -i wlan0 -Dnl80211 -c "$RUN/network.conf" >/dev/null 2>&1 || { cleanup; exit 1; }
    i=0
    while [ "$i" -lt 100 ]; do
        if wpa_cli -p "$CTRL" -i wlan0 status 2>/dev/null | grep -q '^wpa_state=COMPLETED$'; then
            udhcpc -i wlan0 -n -q -t 5 -T 3 -p "$RUN/dhcp.pid" >/dev/null 2>&1 && exit 0
            cleanup; exit 1
        fi
        sleep 0.25; i=$((i+1))
    done
    cleanup; exit 1
    ;;
down) cleanup ;;
*) exit 2 ;;
esac
