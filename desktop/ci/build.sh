#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Configure and build desktop/ with a CMake preset. Runs inside the builder.
#
#   build.sh PRESET        ci or ci-sanitize
#
# The whole output goes to $OUT/logs; on failure the errors are printed again
# at the end, where a person or an agent reading the tail of the job log
# finds them.
set -eu

preset=${1:?usage: build.sh PRESET}
out=$PWD/${OUT:-out/desktop}/logs
mkdir -p "$out"
log=$out/build-$preset.log

cd desktop
status=0
{
    cmake --preset "$preset" \
        && cmake --build --preset "$preset" -- -k 0
} > "$log" 2>&1 || status=$?

if [ "$status" -ne 0 ]; then
    echo "The build failed. Errors (the whole log is in the artifact, logs/build-$preset.log):"
    grep -n -E -A 12 '(error|Error|ERROR)[: ]|CMake Error|undefined reference|FAILED:' "$log" | head -n 400 || tail -n 100 "$log"
    exit "$status"
fi

tail -n 3 "$log"
echo "Warnings in the build log: $(grep -c -E 'warning:' "$log" || true)"
grep -E 'warning:' "$log" | sort -u | head -n 40 || true
