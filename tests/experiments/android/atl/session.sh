#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Inside the D-Bus session of the ATL jobs: a headless sway, a notification
# daemon, the portal's configuration, then the harness. Started by
# run-in-alpine.sh as an ordinary user.
set -u

EXP=/src/tests/experiments/android
EV=/work/evidence

mkdir -p "$HOME/.config/xdg-desktop-portal"
printf '[preferred]\ndefault=gtk\n' > "$HOME/.config/xdg-desktop-portal/portals.conf"

sway -c "$EXP/atl/sway.conf" > "$EV/sway.log" 2>&1 &
for _ in $(seq 1 100); do
	sock=$(ls "$XDG_RUNTIME_DIR" 2>/dev/null | grep -E '^wayland-[0-9]+$' | head -n 1)
	[ -n "$sock" ] && break
	sleep 0.2
done
if [ -z "${sock:-}" ]; then
	echo "sway did not start" | tee -a "$EV/setup.txt"
	tail -n 40 "$EV/sway.log"
	exit 1
fi
export WAYLAND_DISPLAY="$sock"
sleep 1
swaymsg -t get_outputs > "$EV/sway-outputs.json" 2>&1
swaymsg -t get_version >> "$EV/setup.txt" 2>&1

# A headless compositor has no input devices, so its seat offers neither
# keyboard nor pointer and applications never listen for either. Hold one
# virtual keyboard (wtype, asleep) and one virtual pointer for the session.
command -v wtype >/dev/null && (wtype -s 100000000 -k F24 > /dev/null 2>&1 &)
python3 "$EXP/atl/vpointer.py" "$XDG_RUNTIME_DIR/vpointer.cmd" 1280 1100 > "$EV/vpointer.log" 2>&1 &
sleep 1.5
swaymsg -t get_seats > "$EV/sway-seats.json" 2>&1

command -v mako >/dev/null && (mako > "$EV/mako.log" 2>&1 &)
# So that D-Bus-activated services (the portal, its GTK backend, the
# accessibility bus) find the compositor.
dbus-update-activation-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_RUNTIME_DIR SWAYSOCK 2>/dev/null

set -- --apks /work/apks --out "$EV" --scale "${ATL_SCALE:-1}" --title "${ATL_TITLE:-run}"
for lock in /work/locks/*.tsv; do
	[ -f "$lock" ] && set -- "$@" --lock "$lock"
done
[ -n "${ATL_PROBE:-}" ] && [ -f "/work/probe/$ATL_PROBE" ] && set -- "$@" --probe "/work/probe/$ATL_PROBE"
[ "${ATL_LIMIT:-0}" != "0" ] && set -- "$@" --limit "$ATL_LIMIT"
[ -n "${ATL_PROBE_PHASES:-}" ] && set -- "$@" --probe-phases "$ATL_PROBE_PHASES"

python3 "$EXP/atl/harness.py" "$@" 2>&1 | tee "$EV/harness.log"

# Contact sheets: six first screens a sheet, so that a person can look at
# every one.
if ls "$EV"/shots/*-1.png >/dev/null 2>&1; then
	montage -label '%t' "$EV"/shots/*-1.png -tile 6x1 -geometry 300x560+6+6 -background '#202020' -fill white \
		"$EV/sheet-first-%02d.png" 2>> "$EV/setup.txt" ||
		magick montage -label '%t' "$EV"/shots/*-1.png -tile 6x1 -geometry 300x560+6+6 -background '#202020' -fill white \
			"$EV/sheet-first-%02d.png" 2>> "$EV/setup.txt"
fi
swaymsg exit >/dev/null 2>&1
exit 0
