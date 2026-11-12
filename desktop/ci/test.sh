#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Run the unit tests and the QML tests of a built preset, headless. Runs
# inside the builder.
#
#   test.sh PRESET         ci or ci-sanitize
#
# The whole output goes to $OUT/logs; the failures are printed again at the
# end.
set -eu

preset=${1:?usage: test.sh PRESET}
out=$PWD/${OUT:-out/desktop}/logs
mkdir -p "$out"
log=$out/test-$preset.log

cd desktop
status=0
ctest --preset "$preset" --verbose --output-junit "$out/test-$preset.xml" > "$log" 2>&1 || status=$?

# What the tests ran on, and what the walks covered.
grep -E 'TUR-(ENV|A11Y|CONTRAST) ' "$log" | sed -E 's/^[0-9]+: //; s/^QINFO +: [^ ]+ //' | sort -u || true
echo "Test functions: $(grep -c -E '^[0-9]+: PASS ' "$log" || true) passed, $(grep -c -E '^[0-9]+: FAIL! ' "$log" || true) failed, $(grep -c -E '^[0-9]+: SKIP ' "$log" || true) skipped"
grep -E '^[0-9]+: SKIP ' "$log" || true
sed -n '/tests passed,/,$p' "$log"

if [ "$status" -ne 0 ]; then
    echo "Tests failed (the whole log is in the artifact, logs/test-$preset.log):"
    grep -n -E -A 8 '^[0-9]+: (FAIL!|QFATAL|QWARN|XPASS)|Sanitizer|runtime error:|Segmentation|\*\*\*Failed|\*\*\*Exception|\*\*\*Timeout' "$log" | head -n 400 || tail -n 100 "$log"
    exit "$status"
fi
