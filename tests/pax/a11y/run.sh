#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The accessibility test of P@x, inside the image of tests/pax/a11y/
# Containerfile (the `accessibility` job of .github/workflows/pax.yml):
#
#   run.sh PRESET OUT
#
# Builds tur-settings and tur-pax-overlay from desktop/ with the CMake
# preset, and P@x's bridge into Windows applications (services/
# tur-winbridge) and a Windows test window (winwindow.c) with mingw-w64;
# then runs tests/pax/a11y/harness.py, which starts a private session bus,
# the accessibility bus, a headless KWin and the applications, and drives
# P@x (tur-pax with the scripted provider, tur-pax-a11y and tur-runtime,
# built by the job beforehand) through them. Everything goes to OUT.
#
# Runs on the CI runner, inside the container, never on a development
# machine (AGENTS.md).
set -eu

preset=${1:?usage: run.sh PRESET OUT}
out=${2:?usage: run.sh PRESET OUT}
mkdir -p "$out"

echo "== building tur-settings and tur-pax-overlay ($preset)"
if ! (cd desktop && cmake --preset "$preset" && cmake --build --preset "$preset" --target tur-settings tur-pax-overlay) > "$out/build.log" 2>&1; then
    echo "::error::the desktop programs did not build"
    tail -n 120 "$out/build.log"
    exit 1
fi
tail -n 2 "$out/build.log"

echo "== building tur-winbridge.exe and the Windows test window (mingw-w64)"
winbuild="$out/winbuild"
mkdir -p "$winbuild"
if sh services/tur-winbridge/build.sh "$winbuild" > "$out/winbuild.log" 2>&1 &&
    x86_64-w64-mingw32-gcc -O2 -municode -mwindows -o "$winbuild/winwindow.exe" tests/pax/a11y/winwindow.c \
        -lshell32 -luser32 -lgdi32 -lkernel32 >> "$out/winbuild.log" 2>&1; then
    tail -n 12 "$out/winbuild.log"
else
    echo "::error::the bridge or the Windows test window did not build"
    tail -n 60 "$out/winbuild.log"
fi

# Fedora's kwin_wayland carries a file capability that an unprivileged user
# in a container cannot execute; a copy carries none (desktop/ci/
# compositor.sh says more).
kwin_dir=$(mktemp -d)
cp /usr/bin/kwin_wayland "$kwin_dir/kwin_wayland"

cp /usr/share/tur-builder/a11y-packages.tsv "$out/packages.tsv" 2>/dev/null || true
{
    "$kwin_dir/kwin_wayland" --version 2>/dev/null || true
    rpm -q kwin at-spi2-core at-spi2-atk gtk3 gtk4 gnome-text-editor firefox wine qt6-qtbase mingw64-gcc mingw64-crt 2>/dev/null || true
} | tee "$out/versions.txt"

exec python3 tests/pax/a11y/harness.py \
    --out "$out" \
    --build "desktop/build/$preset" \
    --kwin "$kwin_dir/kwin_wayland" \
    --tur-pax services/tur-pax/target/debug/tur-pax \
    --bridge services/tur-pax-a11y/target/debug/tur-pax-a11y \
    --winbuild "$winbuild" \
    --tur-runtime services/tur-runtime/target/debug/tur-runtime
