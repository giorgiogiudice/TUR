#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Run qmllint on every QML module of a built preset. A warning fails
# (desktop-experience.md, 15.2). Runs inside the builder, after build.sh.
#
#   lint.sh PRESET
set -eu

preset=${1:?usage: lint.sh PRESET}
out=$PWD/${OUT:-out/desktop}/logs
mkdir -p "$out"
log=$out/qmllint-$preset.log

cd desktop
status=0
cmake --build --preset "$preset" --target all_qmllint -- -k 0 > "$log" 2>&1 || status=$?

# qmllint's exit status does not report warnings in every version of Qt; its
# output does.
found=$(grep -c -i -E '(warning|error): ' "$log" || true)
if [ "$status" -ne 0 ] || [ "$found" -ne 0 ]; then
    echo "qmllint: $found warning(s) or error(s), build status $status:"
    grep -i -E -A 6 '(warning|error): |FAILED:' "$log" | head -n 400 || true
    echo "-- end of the log (the whole of it is in the artifact, logs/qmllint-$preset.log)"
    tail -n 30 "$log"
    exit 1
fi
tail -n 5 "$log"
echo "qmllint: no warnings"
