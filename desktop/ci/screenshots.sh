#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Render every page of the gallery and the toolchain proof, twice, and
# compare the pictures with the references in desktop/tests/screenshots.
# Runs inside the builder, after build.sh.
#
#   screenshots.sh PRESET
#
# Everything is rendered twice so that a picture which is not repeatable is
# reported as that, and not as a difference from its reference. Results go to
# $OUT/screenshots: actual/, reference/, diff/, proposed/, contact sheets and
# report.md. See desktop/README.md, "Reference images".
set -eu

preset=${1:?usage: screenshots.sh PRESET}
build=$PWD/desktop/build/$preset
out=$PWD/${OUT:-out/desktop}/screenshots
runner=$build/kit/tests/tst_kit
rm -rf "$out"
mkdir -p "$out/actual" "$out/repeat" "$out/logs" "$out/empty-config"

export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QT_FONT_DPI=96 LC_ALL=C.UTF-8
export QML_IMPORT_PATH="$build/qml"
export XDG_CONFIG_HOME="$out/empty-config" XDG_CONFIG_DIRS="$out/empty-config"

status=0

# First prove that the comparison can fail: a green check must mean the
# pictures matched.
python3 desktop/ci/compare_screenshots.py --self-test

# render DIRECTORY SCALE INPUT
render() {
    name=$(basename "$3" .qml)-$2-$(basename "$1")
    if ! TUR_SCREENSHOT_DIR="$1" QT_SCALE_FACTOR="$2" "$runner" -input "$3" > "$out/logs/$name.log" 2>&1; then
        status=1
        echo "::error::rendering failed: $name"
        grep -E -A 8 '^(FAIL!|QFATAL|QWARN)' "$out/logs/$name.log" | head -n 120 || tail -n 60 "$out/logs/$name.log"
    fi
    grep -E '^Totals|TUR-ENV' "$out/logs/$name.log" | sed -E 's/^QINFO +: [^ ]+ //' || true
}

for pass in actual repeat; do
    render "$out/$pass" 1 desktop/ci/probe/tst_probe.qml
    render "$out/$pass" 1 desktop/kit/tests/shots/render_gallery.qml
    render "$out/$pass" 1.5 desktop/kit/tests/shots/render_gallery.qml
done

python3 desktop/ci/compare_screenshots.py \
    --actual "$out/actual" --repeat "$out/repeat" \
    --reference desktop/tests/screenshots --out "$out" || status=1

rm -rf "$out/empty-config"
exit "$status"
