#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds the Wine modules the KMDF experiment replaces, from Wine's own
# source at the version Fedora 45 ships (wine-11.20; Fedora's Staging patch
# set changes none of these files) with TUR's patch set applied
# (third-party/wine-patches, LGPL-2.1-or-later, applied in the order of its
# `series` file). Only these modules are built, not all of Wine:
#
#   ntoskrnl.exe   Wine's user-space kernel, with the patches
#   wdfldr.sys     the KMDF loader the patch set adds
#   wineusb.sys    Wine's USB bus driver, with the patches (the PE half and
#                  its Unix half wineusb.so both go in the overlay)
#
#   tests/windows-drivers/kmdf/build-wine.sh OUTPUT_DIR
#
# Output: OUTPUT_DIR/wine-overlay/{ntoskrnl.exe,wdfldr.sys} (PE files to put
# over Fedora's in the experiment's throw-away container), OUTPUT_DIR/
# wine-src (the patched tree, for imports.py), OUTPUT_DIR/wine-build.log.
# Also checks that the patches apply to Wine's master branch (pinned).
set -euo pipefail

WINE_REPO=https://gitlab.winehq.org/wine/wine.git
WINE_TAG=wine-11.20
WINE_COMMIT=7298a818b79446bae153b8d5aa27235ab3703bf9
WINE_MASTER=cc5d1d86ee7d60649e6f858106db046c77c8b12c

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "${here}/../../.." && pwd)"
patches="${repo}/third-party/wine-patches"
out="$(mkdir -p "${1:?output directory required}" && cd "$1" && pwd)"
src="${out}/wine-src"
build="${out}/wine-build"
overlay="${out}/wine-overlay"
rm -rf "${src}" "${build}" "${overlay}"
mkdir -p "${build}" "${overlay}"

git init -q "${src}"
git -C "${src}" remote add origin "${WINE_REPO}"
git -C "${src}" fetch -q --depth 1 origin "refs/tags/${WINE_TAG}"
git -C "${src}" checkout -q FETCH_HEAD
[ "$(git -C "${src}" rev-parse HEAD)" = "${WINE_COMMIT}" ] || {
    echo "wine: ${WINE_TAG} is $(git -C "${src}" rev-parse HEAD), expected ${WINE_COMMIT}" >&2
    exit 1
}
echo "wine: ${WINE_TAG} ${WINE_COMMIT}"

mapfile -t series < <(grep -vE '^\s*(#|$)' "${patches}/series")
for p in "${series[@]}"; do
    git -C "${src}" apply --whitespace=nowarn "${patches}/${p}"
    echo "applied: ${p}"
done

# The same patches against Wine's master branch (pinned): an upstream
# merge request would be made against it.
master="${out}/wine-master-check"
rm -rf "${master}"
git init -q "${master}"
git -C "${master}" remote add origin "${WINE_REPO}"
if git -C "${master}" fetch -q --depth 1 origin "${WINE_MASTER}" 2>/dev/null; then
    git -C "${master}" checkout -q FETCH_HEAD
    for p in "${series[@]}"; do
        if git -C "${master}" apply --check "${patches}/${p}" 2>/dev/null; then
            echo "applies to master ${WINE_MASTER:0:8}: ${p}"
            git -C "${master}" apply "${patches}/${p}"
        else
            echo "does NOT apply to master ${WINE_MASTER:0:8}: ${p}"
        fi
    done
else
    echo "master ${WINE_MASTER}: could not be fetched; not checked"
fi
rm -rf "${master}"

# configure.ac gained a module: regenerate configure.
(cd "${src}" && autoconf -f) >"${out}/wine-autoconf.log" 2>&1

cd "${build}"
"${src}/configure" --enable-archs=x86_64 --without-x --without-freetype --without-wayland \
    --without-vulkan --without-opengl --without-gstreamer --without-pulse --without-alsa \
    --without-cups --without-sane --without-gphoto --without-v4l2 --without-krb5 --without-netapi \
    --without-pcap --without-sdl --without-udev --without-dbus --without-capi \
    --without-ffmpeg --without-gssapi --without-inotify --without-fontconfig --without-gnutls \
    --without-xinerama --without-xrandr --without-xshape --without-xxf86vm --without-xinput2 \
    --without-xcomposite --without-xcursor --without-xfixes --without-xinput --without-xrender \
    --without-xshm --without-osmesa --without-opencl --disable-tests \
    >"${out}/wine-configure.log" 2>&1 || { tail -n 40 "${out}/wine-configure.log"; exit 1; }
echo "configure: ok"
make -j"$(nproc)" dlls/ntoskrnl.exe/all dlls/wdfldr.sys/all dlls/wineusb.sys/all dlls/kernelbase/all >"${out}/wine-build.log" 2>&1 ||
    { grep -E 'error|Error' "${out}/wine-build.log" | head -n 60; exit 1; }
for m in ntoskrnl.exe wdfldr.sys wineusb.sys; do
    f="$(find "${build}/dlls/${m}" -name "${m}" -path '*x86_64-windows*' | head -n 1)"
    [ -n "${f}" ] || { echo "not built: ${m}" >&2; exit 1; }
    cp "${f}" "${overlay}/${m}"
    echo "built: ${m} $(stat -c %s "${overlay}/${m}") bytes"
done
# kernelbase.dll (patch 0016, DeviceIoControl without an OVERLAPPED). The
# harness installs it for the FX2 scenario only: Fedora ships Wine Staging's
# kernelbase, and this is plain 11.20 plus the patch.
f="$(find "${build}/dlls/kernelbase" -name kernelbase.dll -path '*x86_64-windows*' | head -n 1)"
if [ -n "${f}" ]; then
    cp "${f}" "${overlay}/kernelbase.dll"
    echo "built: kernelbase.dll $(stat -c %s "${overlay}/kernelbase.dll") bytes"
else
    echo "not built: kernelbase.dll"
fi
# wineusb.sys's Unix half (libusb); patches 0012-0013 change it, so it goes
# in the overlay too (Wine's x86_64-unix directory).
f="$(find "${build}/dlls/wineusb.sys" -name wineusb.so | head -n 1)"
if [ -n "${f}" ]; then
    cp "${f}" "${overlay}/wineusb.so"
    echo "built: wineusb.so $(stat -c %s "${overlay}/wineusb.so") bytes"
else
    echo "not built: wineusb.so (libusb not found by configure?)"
fi
