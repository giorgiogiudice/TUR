#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The accessibility test of P@x, inside the image of tests/pax/a11y/
# Containerfile (the `accessibility` job of .github/workflows/pax.yml):
#
#   run.sh PRESET OUT
#
# Builds tur-settings and tur-pax-overlay from desktop/ with the CMake
# preset, then runs tests/pax/a11y/harness.py, which starts a private
# session bus, the accessibility bus, a headless KWin and the applications,
# and drives P@x (tur-pax with the scripted provider and tur-pax-a11y, both
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

# Fedora's kwin_wayland carries a file capability that an unprivileged user
# in a container cannot execute; a copy carries none (desktop/ci/
# compositor.sh says more).
kwin_dir=$(mktemp -d)
cp /usr/bin/kwin_wayland "$kwin_dir/kwin_wayland"

cp /usr/share/tur-builder/a11y-packages.tsv "$out/packages.tsv" 2>/dev/null || true
{
    "$kwin_dir/kwin_wayland" --version 2>/dev/null || true
    rpm -q kwin at-spi2-core at-spi2-atk gtk3 gtk4 gnome-text-editor firefox wine qt6-qtbase 2>/dev/null || true
} | tee "$out/versions.txt"

exec python3 tests/pax/a11y/harness.py \
    --out "$out" \
    --build "desktop/build/$preset" \
    --kwin "$kwin_dir/kwin_wayland" \
    --tur-pax services/tur-pax/target/debug/tur-pax \
    --bridge services/tur-pax-a11y/target/debug/tur-pax-a11y
