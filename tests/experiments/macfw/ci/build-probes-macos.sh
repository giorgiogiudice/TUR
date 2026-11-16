#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# On GitHub's macOS runner: build TUR's probes and TUR's own ladder rungs
# with Xcode's public toolchain, as universal binaries (arm64 and x86_64).
#
#     ci/build-probes-macos.sh OUT
#
# OUT/bin/<probe>        fnd win view ctl tbl evt swak sui, and mfwin
# OUT/apps/<rung>.app    rung1-hello rung2-form rung6-swift rung8-swiftui
# OUT/build-manifest.txt for each binary: slices, linked libraries, SHA-256
#
# What leaves the runner (as a workflow artifact) is only these programs:
# TUR's code, compiled. They link Apple's frameworks dynamically, by name;
# no Apple library, header or SDK file is copied into them or next to them.
set -euo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
OUT=$(mkdir -p "$1" && cd "$1" && pwd)
mkdir -p "$OUT/bin" "$OUT/apps"
LOG="$OUT/build.log"
: > "$LOG"

CFLAGS=(-arch arm64 -arch x86_64 -fobjc-arc -O1 -g0 -Wall -Wno-unused-function -Wno-deprecated-declarations
        -I "$HERE/probes")

objc() { # objc OUT MIN SOURCE... -- frameworks
    local out=$1 min=$2; shift 2
    echo "== clang $out" >> "$LOG"
    xcrun clang "${CFLAGS[@]}" -mmacosx-version-min="$min" -o "$out" "$@" >> "$LOG" 2>&1
}

swift_universal() { # swift_universal OUT MIN MODE SOURCE...   (MODE: script | library)
    local out=$1 min=$2 mode=$3; shift 3
    local tmp parts=()
    tmp=$(mktemp -d)
    local srcs=()
    for s in "$@"; do
        if [ "$mode" = script ] && [ "$s" = "$1" ]; then cp "$s" "$tmp/main.swift"; srcs+=("$tmp/main.swift")
        else srcs+=("$s"); fi
    done
    local extra=()
    [ "$mode" = library ] && extra=(-parse-as-library)
    for arch in arm64 x86_64; do
        echo "== swiftc $out ($arch)" >> "$LOG"
        xcrun swiftc -swift-version 5 -strict-concurrency=minimal -O -target "$arch-apple-macos$min" "${extra[@]}" \
            -o "$tmp/out.$arch" "${srcs[@]}" >> "$LOG" 2>&1
        parts+=("$tmp/out.$arch")
    done
    lipo -create -output "$out" "${parts[@]}"
    rm -rf "$tmp"
}

bundle() { # bundle NAME EXECUTABLE MIN
    local name=$1 exe=$2 min=$3 app="$OUT/apps/$1.app"
    [ -f "$exe" ] || { echo "not built: $name" >> "$LOG"; return 0; }
    mkdir -p "$app/Contents/MacOS"
    mv "$exe" "$app/Contents/MacOS/$name"
    cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>$name</string>
  <key>CFBundleIdentifier</key><string>org.tur.macfw.$name</string>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>$min</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
    codesign --force --sign - "$app" >> "$LOG" 2>&1
}

# A failed build is recorded and the next one is tried; the script fails at
# the end. step always returns 0, so `step a && step b` runs b regardless
# and b checks its own input.
status=0
step() { "$@" || { echo "FAILED: $*" | tee -a "$LOG"; status=1; }; }

# Probes (Objective-C): one program per area.
for p in "fnd foundation_basic.m" "win appkit_window.m" "view appkit_view.m" "ctl appkit_controls.m" \
         "tbl appkit_table_text.m" "evt appkit_events.m"; do
    set -- $p
    step objc "$OUT/bin/$1" 11.0 "$HERE/probes/$2" -framework AppKit -framework Foundation
done
step objc "$OUT/bin/mfwin" 11.0 "$HERE/probes/mfwin.m" -framework CoreGraphics -framework Foundation

# Probes (Swift).
step swift_universal "$OUT/bin/swak" 14.0 script "$HERE/probes/swift_appkit.swift" "$HERE/probes/MF.swift"
step swift_universal "$OUT/bin/sui" 14.0 script "$HERE/probes/swiftui_basic.swift" "$HERE/probes/MF.swift"

# TUR's own ladder rungs.
L="$HERE/ladder/own"
step objc "$OUT/rung1-hello" 11.0 "$L/rung1-hello.m" -framework AppKit && step bundle rung1-hello "$OUT/rung1-hello" 11.0
step objc "$OUT/rung2-form" 11.0 "$L/rung2-form.m" -framework AppKit && step bundle rung2-form "$OUT/rung2-form" 11.0
step swift_universal "$OUT/rung6-swift" 14.0 script "$L/rung6-swift.swift" && step bundle rung6-swift "$OUT/rung6-swift" 14.0
step swift_universal "$OUT/rung8-swiftui" 14.0 library "$L/rung8-swiftui.swift" && step bundle rung8-swiftui "$OUT/rung8-swiftui" 14.0

# What was built and what it links against.
{
    echo "# xcode: $(xcodebuild -version | tr '\n' ' ')"
    echo "# sdk: $(xcrun --sdk macosx --show-sdk-version)"
    for f in "$OUT"/bin/* "$OUT"/apps/*.app/Contents/MacOS/*; do
        [ -f "$f" ] || continue
        echo "## ${f#$OUT/}"
        echo "slices: $(lipo -archs "$f")"
        echo "sha256: $(shasum -a 256 "$f" | cut -d' ' -f1)"
        otool -L -arch x86_64 "$f" | tail -n +2 | sed 's/^/  /'
    done
} > "$OUT/build-manifest.txt"
exit $status
