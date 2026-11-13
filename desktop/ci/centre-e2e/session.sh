#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The end-to-end session, under its own session bus and the X server of
# inside.sh:
#
#  1. record.py records real runs of tur-apps for the fixtures, as a user of
#     its own;
#  2. the real application centre (its QML module and C++, run by tst_centre
#     on desktop/apps/tur-centre/tests/e2e/*.qml) is driven through its own
#     controls, as a fresh user: switch Flathub on in Sources; open
#     Chromium B.S.U.'s page; Install; read the plan; confirm; watch the
#     progress; Open (watch.sh finds the game's window and takes pictures of
#     the screen); the Installed view; then, after flatpak has put an
#     earlier commit in place, Update in the Updates view; then Remove,
#     keeping the data; then Install again and Cancel;
#  3. what is on disk afterwards is checked here, from outside the centre.
#
# Every picture is in $OUT/shots, every log in $OUT/logs, and the checks in
# $OUT/summary.md.
set -u

out=${OUT:?}
work=/var/tmp/centre-e2e
rm -rf "$work"
mkdir -p "$work"
export XDG_RUNTIME_DIR=$work/runtime
mkdir -m 0700 "$XDG_RUNTIME_DIR"
app=net.sourceforge.chromium-bsu
summary=$out/summary.md
failed=0

check() {
    # check NAME COMMAND...
    name=$1
    shift
    if "$@" > /dev/null 2>&1; then
        echo "| $name | pass |" >> "$summary"
        echo "[PASS] $name"
    else
        echo "| $name | **FAIL** |" >> "$summary"
        echo "[FAIL] $name"
        failed=1
    fi
}

{
    echo "### The application centre, end to end"
    echo
    echo "| Check | Result |"
    echo "|---|---|"
} > "$summary"

# --- 1. Recordings for the fixtures ------------------------------------------

echo "::group::Recording tur-apps for the fixtures"
python3 desktop/ci/centre-e2e/record.py --tur-apps "$TUR_APPS" --catalogue catalogue \
    --fakes services/tur-apps/tests/fakes --home "$work/record-home" --out "$out/fixtures" \
    > "$out/logs/record.log" 2>&1
record=$?
cat "$out/logs/record.log"
echo "::endgroup::"
check "the recordings were made, and every expectation of record.py was met" test "$record" -eq 0
rm -rf "$work/record-home"

# --- 2. The application centre, driven through its controls ------------------

export HOME=$work/home
export XDG_DATA_HOME=$HOME/.local/share XDG_CONFIG_HOME=$HOME/.config XDG_CACHE_HOME=$HOME/.cache
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
build=$PWD/desktop/build/ci
export QML_IMPORT_PATH=$build/qml QT_QPA_PLATFORM=xcb QT_QUICK_BACKEND=software QT_FONT_DPI=96 LC_ALL=C.UTF-8
export TUR_SCREENSHOT_DIR=$out/shots
export TUR_CENTRE_E2E_APPS=$TUR_APPS TUR_CENTRE_E2E_CATALOGUE_PROGRAM=$TUR_CATALOGUE
export TUR_CENTRE_E2E_CATALOGUE=$PWD/catalogue

part() {
    # part NAME: one file of desktop/apps/tur-centre/tests/e2e
    echo "::group::The application centre: $1"
    "$build/apps/tur-centre/tests/tst_centre" -input "desktop/apps/tur-centre/tests/e2e/$1.qml" \
        > "$out/logs/$1.log" 2>&1
    result=$?
    grep -E '^(PASS|FAIL!|QWARN|QDEBUG|QINFO|Totals)|E2E ' "$out/logs/$1.log" | head -n 200
    echo "::endgroup::"
    check "the centre: $1 (log logs/$1.log)" test "$result" -eq 0
    return 0
}

sh desktop/ci/centre-e2e/watch.sh "$app" "Chromium" > "$out/logs/watch.log" 2>&1 &
watcher=$!
part e2e_install
wait "$watcher"
cat "$out/logs/watch.log"
check "the game's window appeared after Open" grep -q '^found' "$out/logs/watch.log"
check "flatpak lists the application for this user" sh -c "flatpak list --user --app --columns=application | grep -qx $app"
check "nothing was installed system-wide" sh -c "test -z \"\$(flatpak list --system --app --columns=application 2>/dev/null)\""

# An earlier commit, so that the Updates view has something to update.
reference=app/$app/x86_64/stable
flatpak remote-info --user --log flathub "$reference" > "$out/logs/remote-log.txt" 2>&1
older=$(grep -E '^[[:space:]]*Commit:' "$out/logs/remote-log.txt" | awk '{print $2}' | awk '!seen[$0]++' | sed -n 2p)
# What the move to an earlier commit brings in (an earlier runtime) is this
# script's doing, not the centre's: it is not counted as left behind below.
flatpak list --user --columns=ref | sort > "$work/refs-before-downgrade"
if [ -n "$older" ]; then
    flatpak update --user --noninteractive --commit="$older" "$reference" > "$out/logs/downgrade.log" 2>&1
    check "flatpak put the earlier commit $older in place" sh -c "flatpak info --user --show-commit $reference | grep -qx $older"
    export TUR_CENTRE_E2E_UPDATE_EXPECTED=1
else
    echo "| Flathub has no earlier commit of $app: the update finds nothing to update | note |" >> "$summary"
    export TUR_CENTRE_E2E_UPDATE_EXPECTED=0
fi
flatpak list --user --columns=ref | sort > "$work/refs-after-downgrade"
comm -13 "$work/refs-before-downgrade" "$work/refs-after-downgrade" > "$out/logs/brought-in-by-the-downgrade.txt"
part e2e_update

part e2e_remove

# --- 3. What is on disk afterwards ---------------------------------------------

check "its data is still there after Remove (kept by default)" test -d "$HOME/.var/app/$app"
flatpak list --user --columns=ref,origin > "$out/logs/left-at-the-end.txt" 2>&1
flatpak list --user --columns=ref | sort > "$work/refs-at-the-end"
comm -23 "$work/refs-at-the-end" "$out/logs/brought-in-by-the-downgrade.txt" > "$work/left-by-the-centre"
check "no reference the centre installed is left after Remove and the cancel" test ! -s "$work/left-by-the-centre"
check "no deployment of the application is left" test ! -e "$XDG_DATA_HOME/flatpak/app/$app"
check "no record of tur-apps is left" test ! -e "$XDG_DATA_HOME/tur/apps/installed/$app.json"
import -window root "$out/shots/e2e-screen-at-the-end.png" 2>/dev/null || true

echo
cat "$summary"
exit "$failed"
