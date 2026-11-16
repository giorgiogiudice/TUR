#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The candidate: on a GitHub Linux runner, install Darling's packages, give
# it a private X display with a window manager, and run the oracle's probe
# binaries and ladder applications under Darling.
#
#     ci/candidate-linux.sh DEBS ORACLE OUT RUNGS
#
# DEBS    Darling's packages (ci/darling-get.sh)
# ORACLE  the oracle job's artifact: bin/ and apps/, built on macOS
# OUT     results-darling-<arch>.json, ladder-darling/, env.txt, darling logs
#
# X11 first (Xvfb :99 with xfwm4, as Darling's own GUI tests use); the
# Wayland backend (headless sway) is a later variant. Ubuntu's runner image,
# not Fedora: Darling publishes Debian packages only, and Darling, not the
# distribution, is what is measured. Report section 6.4.
set -uo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
DEBS=$(cd "$1" && pwd)
ORACLE=$(cd "$2" && pwd)
OUT=$(mkdir -p "$3" && cd "$3" && pwd)
RUNGS=${4:-}

sudo apt-get update -q
sudo apt-get install -y -q xvfb xfwm4 wmctrl xdotool imagemagick x11-utils dbus-x11
sudo apt-get install -y -q "$DEBS"/*.deb
# Darling maps low addresses for 32-bit and old programs (AppDB, 2026-11-15).
sudo sysctl -w vm.mmap_min_addr=4096 || true

export DISPLAY=:99
Xvfb :99 -screen 0 1920x1080x24 +extension RANDR > "$OUT/xvfb.log" 2>&1 &
sleep 2
dbus-launch xfwm4 > "$OUT/xfwm4.log" 2>&1 &
sleep 2

{
    echo "date: $(date -u +%FT%TZ)"
    uname -a
    grep PRETTY_NAME /etc/os-release
    dpkg -l | grep -i darling | awk '{print $2, $3, $4}'
    cat "$DEBS/darling-get.txt" 2>/dev/null
    timeout -s KILL 300 darling shell sw_vers </dev/null > "$OUT/sw_vers.txt" 2>&1
    echo "darling sw_vers (rc $?):"; cat "$OUT/sw_vers.txt"
} > "$OUT/env.txt" 2>&1
cat "$OUT/env.txt"

chmod +x "$ORACLE"/bin/* "$ORACLE"/apps/*.app/Contents/MacOS/* 2>/dev/null || true
arch=$(uname -m)
python3 "$HERE/harness/run_probes.py" --side darling --bin "$ORACLE/bin" --out "$OUT/results-darling-$arch.json" \
    --display :99 --timeout 90
timeout -s KILL 60 darling shutdown </dev/null || true

python3 "$HERE/harness/ladder.py" --side darling --apps "$ORACLE/apps" --ladder "$HERE/ladder/ladder.json" \
    --out "$OUT/ladder-darling" --rungs "$RUNGS" --display :99
timeout -s KILL 60 darling shutdown </dev/null || true
# Darling's own logs from the prefix, for the fix loop.
cp -r "$HOME/.darling/private/var/log" "$OUT/darling-var-log" 2>/dev/null || true
exit 0
