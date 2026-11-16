#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The oracle: on GitHub's macOS runner (Apple hardware, Apple's macOS), build
# the probes and the ladder, then run them and record what macOS does.
#
#     ci/oracle-macos.sh OUT RUNGS
#
# OUT/env.txt                    macOS, Xcode, machine, Rosetta
# OUT/bin, OUT/apps              what the candidate will run (see
#                                build-probes-macos.sh, build_ladder_macos.py)
# OUT/results-macos-arm64.json   the probes, natively
# OUT/results-macos-x86_64.json  the probes' x86_64 slice under Rosetta, when
#                                the runner has Rosetta (the candidate on
#                                x86-64 Linux runs that slice)
# OUT/ladder-macos/              the ladder on macOS: scores, window lists,
#                                screenshots where permitted
#
# Every step records its result and the script goes on; it fails only when
# nothing could be built.
set -uo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
OUT=$(mkdir -p "$1" && cd "$1" && pwd)
RUNGS=${2:-}

{
    echo "date: $(date -u +%FT%TZ)"
    sw_vers
    echo "machine: $(uname -m) $(sysctl -n hw.model 2>/dev/null)"
    echo "cpu: $(sysctl -n machdep.cpu.brand_string 2>/dev/null)"
    xcodebuild -version
    echo "sdk: $(xcrun --sdk macosx --show-sdk-version)"
    echo "swift: $(xcrun swift --version 2>&1 | head -1)"
    if arch -x86_64 /usr/bin/true 2>/dev/null; then echo "rosetta: yes"; else echo "rosetta: no"; fi
    echo "screen: $(system_profiler SPDisplaysDataType 2>/dev/null | grep -m1 Resolution | xargs)"
} > "$OUT/env.txt" 2>&1
cat "$OUT/env.txt"

bash "$HERE/ci/build-probes-macos.sh" "$OUT" || echo "some probes or own rungs failed to build (build.log)"
python3 "$HERE/ci/build_ladder_macos.py" --ladder "$HERE/ladder/ladder.json" --out "$OUT" --rungs "$RUNGS" \
    || echo "ladder build reported an error"
ls "$OUT/bin" "$OUT/apps" || true
[ -n "$(ls -A "$OUT/bin" 2>/dev/null)" ] || { echo "nothing was built"; exit 1; }

# Disk images of two of the built applications, for the macOS route's
# integration test (tests/macos-darling, workflow macos-darling), which
# reuses this job's artifact instead of a macOS job of its own: one HFS+
# image (UDZO, zlib) and one APFS image (ULFO, LZFSE) of each, made by
# macOS's own hdiutil from TUR's builds. A failure is recorded, never fatal.
mkdir -p "$OUT/dmg"
for app in "rung1-hello.app" "Table Tool.app"; do
    [ -d "$OUT/apps/$app" ] || continue
    name=$(basename "$app" .app | tr ' ' '-')
    hdiutil create -quiet -volname "$name" -srcfolder "$OUT/apps/$app" -fs HFS+ -format UDZO \
        "$OUT/dmg/$name-hfs.dmg" || echo "hdiutil HFS+ UDZO $app failed" >> "$OUT/dmg/errors.txt"
    hdiutil create -quiet -volname "$name" -srcfolder "$OUT/apps/$app" -fs APFS -format ULFO \
        "$OUT/dmg/$name-apfs.dmg" || echo "hdiutil APFS ULFO $app failed" >> "$OUT/dmg/errors.txt"
done
ls -l "$OUT/dmg" || true

python3 "$HERE/harness/run_probes.py" --side macos --bin "$OUT/bin" --out "$OUT/results-macos-arm64.json"
if grep -q '^rosetta: yes' "$OUT/env.txt"; then
    python3 "$HERE/harness/run_probes.py" --side macos --arch x86_64 --bin "$OUT/bin" \
        --out "$OUT/results-macos-x86_64.json"
fi

python3 "$HERE/harness/ladder.py" --side macos --apps "$OUT/apps" --ladder "$HERE/ladder/ladder.json" \
    --out "$OUT/ladder-macos" --rungs "$RUNGS" --mfwin "$OUT/bin/mfwin"
exit 0
