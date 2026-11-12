#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The `compositor` job (desktop-experience.md, 15.3): TUR's desktop programs
# under a real KWin. Runs inside the builder, after build.sh, with a render
# device for KWin's software renderer passed in (/dev/udmabuf; see the job in
# .github/workflows/desktop.yml).
#
#   compositor.sh PRESET
#
# For each arrangement below it starts a private session bus and
# kwin_wayland with its virtual backend (no display, no GPU: Mesa's software
# renderer), then desktop/tests/compositor's tst_compositor, which starts
# tur-settings and tur-pax-overlay as Wayland clients and tests them. The
# KWin is the one the builder image has from Fedora 45, the same package the
# TUR image ships.
#
# Everything goes to $OUT/compositor: each arrangement's KWin log, test log
# (JUnit and text) and screenshots, and report.md.
set -eu

preset=${1:?usage: compositor.sh PRESET}
build=$PWD/desktop/build/$preset
out=$PWD/${OUT:-out/desktop}/compositor
program=$build/tests/compositor/tst_compositor
rm -rf "$out"
mkdir -p "$out"

# Fedora's kwin_wayland carries a file capability (CAP_SYS_NICE, for its
# real-time thread). A container does not let an unprivileged user run a
# program with a capability outside its bounding set: exec fails with
# "Operation not permitted". A copy of the program carries no capability;
# KWin runs without real-time scheduling, which it allows for.
kwin=$(mktemp -d)/kwin_wayland
cp /usr/bin/kwin_wayland "$kwin"
getcap /usr/bin/kwin_wayland 2>/dev/null || true

echo "== the compositor"
"$kwin" --version || true
rpm -q kwin kwin-wayland kwin-common kwayland layer-shell-qt mesa-libEGL mesa-dri-drivers 2>/dev/null || true
ls -l /dev/udmabuf /dev/dri 2>&1 || true

status=0

# run LABEL SCALE OUTPUTS VARIANT
run() {
    label=$1
    scale=$2
    outputs=$3
    variant=$4
    echo "== $label: scale $scale, $outputs output(s), tur-settings in the $variant variant"

    runtime=$(mktemp -d)
    chmod 700 "$runtime"
    mkdir -p "$runtime/config"
    bus="unix:path=$runtime/bus"
    dbus-daemon --session --nofork --nopidfile --address="$bus" > "$out/$label-dbus.log" 2>&1 &
    daemon=$!
    tries=0
    while [ ! -S "$runtime/bus" ] && [ "$tries" -lt 100 ]; do
        sleep 0.1
        tries=$((tries + 1))
    done

    # The scale. kwin_wayland's --scale is not what an output ends up with:
    # KWin 6.8 takes each output's scale from its own output configuration
    # (src/outputconfigurationstore.cpp), choosing one from the physical size
    # where it has none, and a virtual output has no physical size, so 1. So
    # KWin is started once to write its configuration, which then gets the
    # scale asked for, and is started again.
    if [ "$scale" != 1 ]; then
        XDG_RUNTIME_DIR=$runtime XDG_CONFIG_HOME=$runtime/config DBUS_SESSION_BUS_ADDRESS=$bus \
            "$kwin" --virtual --width 1280 --height 800 --output-count "$outputs" \
            --socket wayland-first --no-lockscreen --no-global-shortcuts --no-kactivities \
            > "$out/$label-kwin-first.log" 2>&1 &
        first=$!
        tries=0
        while [ ! -f "$runtime/config/kwinoutputconfig.json" ] && [ "$tries" -lt 100 ]; do
            sleep 0.1
            tries=$((tries + 1))
        done
        sleep 1
        kill "$first" 2>/dev/null || true
        wait "$first" 2>/dev/null || true
        if [ -f "$runtime/config/kwinoutputconfig.json" ]; then
            python3 - "$runtime/config/kwinoutputconfig.json" "$scale" <<'PY'
import json, sys
path, scale = sys.argv[1], float(sys.argv[2])
config = json.load(open(path))
for section in config:
    if section.get("name") == "outputs":
        for output in section["data"]:
            output["scale"] = scale
json.dump(config, open(path, "w"), indent=4)
print(f"kwinoutputconfig.json: every output at scale {scale}")
PY
        else
            echo "::warning::$label: KWin wrote no output configuration; the scale stays KWin's choice"
        fi
    fi

    # Nothing of the machine's or the runner's configuration: an empty
    # configuration directory, and only what is set here.
    XDG_RUNTIME_DIR=$runtime XDG_CONFIG_HOME=$runtime/config DBUS_SESSION_BUS_ADDRESS=$bus \
        "$kwin" --virtual --width 1280 --height 800 --scale "$scale" --output-count "$outputs" \
        --socket wayland-tur --no-lockscreen --no-global-shortcuts --no-kactivities \
        > "$out/$label-kwin.log" 2>&1 &
    compositor=$!
    tries=0
    while [ ! -S "$runtime/wayland-tur" ] && [ "$tries" -lt 300 ]; do
        if ! kill -0 "$compositor" 2>/dev/null; then
            break
        fi
        sleep 0.1
        tries=$((tries + 1))
    done
    if [ ! -S "$runtime/wayland-tur" ]; then
        echo "::error::$label: kwin_wayland did not open its socket"
        tail -n 80 "$out/$label-kwin.log"
        kill "$compositor" "$daemon" 2>/dev/null || true
        status=1
        return
    fi
    sleep 2

    # The clients: Wayland, Qt Quick's software renderer (there is no GPU),
    # a 24 px cursor, nothing from the runner's configuration.
    if ! XDG_RUNTIME_DIR=$runtime XDG_CONFIG_HOME=$runtime/config DBUS_SESSION_BUS_ADDRESS=$bus \
        WAYLAND_DISPLAY=wayland-tur QT_QPA_PLATFORM=wayland QT_QUICK_BACKEND=software \
        XCURSOR_SIZE=24 LC_ALL=C.UTF-8 ASAN_OPTIONS=detect_leaks=0 \
        TUR_COMPOSITOR_OUT=$out TUR_COMPOSITOR_LABEL=$label TUR_COMPOSITOR_VARIANT=$variant \
        timeout 600 "$program" -o "$out/$label.xml,junitxml" -o "-,txt" > "$out/$label-test.log" 2>&1; then
        status=1
        echo "::error::$label: the compositor tests failed"
    fi
    grep -a -E '^(PASS|FAIL!|SKIP|XFAIL|XPASS|Totals)' "$out/$label-test.log" || true
    grep -a -E -A 6 '^FAIL!' "$out/$label-test.log" | head -n 80 || true
    grep -a 'TUR-KWIN' "$out/$label-test.log" | sed "s/^/$label: /" >> "$out/observations.txt" || true

    kill "$compositor" 2>/dev/null || true
    wait "$compositor" 2>/dev/null || true
    kill "$daemon" 2>/dev/null || true
    wait "$daemon" 2>/dev/null || true
    # KWin's own warnings, for the record.
    grep -a -i -E 'warning|error|fail' "$out/$label-kwin.log" | sort | uniq -c | sort -rn | head -n 15 || true
    rm -rf "$runtime"
}

run dark-1x 1 1 dark
run light-1x 1 1 light
run dark-1.5x 1.5 1 dark
run light-1.5x 1.5 1 light
run two-outputs-1x 1 2 dark

{
    echo "### Under KWin"
    echo
    echo '```'
    cat "$out/observations.txt" 2>/dev/null || echo "(nothing was observed)"
    echo '```'
} > "$out/report.md"
cat "$out/report.md"
exit "$status"
