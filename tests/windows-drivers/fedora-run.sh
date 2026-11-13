#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The windows-drivers experiment inside a Fedora 45 container, with Fedora
# 45's own Wine (the Wine TUR would ship). Started by the workflow as
#
#   docker run --privileged --network host -v /dev:/dev -v /sys:/sys \
#       -v /run/udev:/run/udev:ro -v CHECKOUT:/src:ro -v WORK:/work \
#       fedora:45@sha256:... bash /src/tests/windows-drivers/fedora-run.sh
#
# after gadget/gadget.sh has made the virtual USB devices on the runner. The
# container is privileged only so that it sees the runner's devices and can
# change their ACLs (what a TUR service would do); Wine and every Windows
# program run as the unprivileged user "tur". Packages are Fedora's only.
set -euo pipefail

src=/src/tests/windows-drivers
work=/work
mkdir -p "${work}/evidence" "${work}/build"

dnf -y --setopt=install_weak_deps=False install \
    wine mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt \
    acl bubblewrap util-linux python3 git-core findutils procps-ng libusb1 \
    >"${work}/evidence/dnf.log" 2>&1 || { tail -n 50 "${work}/evidence/dnf.log"; exit 1; }

id tur >/dev/null 2>&1 || useradd -m -u 1001 tur

{
    . /etc/os-release
    echo "container: ${PRETTY_NAME}"
    echo "kernel (the runner's): $(uname -r)"
    echo "wine: $(wine --version 2>&1)"
    echo "packages:"
    rpm -q wine wine-core wine-filesystem libusb1 bubblewrap mingw64-gcc mingw64-headers mingw64-crt \
        mingw64-binutils systemd-libs | sed 's/^/  /'
    echo "wine drivers (*.sys) shipped:"
    ls /usr/lib64/wine/x86_64-windows/*.sys 2>/dev/null | xargs -n1 basename | tr '\n' ' '
    echo
    echo "wine unix libraries for drivers:"
    ls /usr/lib64/wine/x86_64-unix/ 2>/dev/null | grep -E 'wineusb|winebus|hid' | tr '\n' ' '
    echo
    echo "wineusb.so links:"
    ldd /usr/lib64/wine/x86_64-unix/wineusb.so 2>&1 | grep -E 'usb|udev' | sed 's/^/  /'
} | tee "${work}/evidence/versions.txt"

echo "== build"
bash "${src}/build.sh" "${work}/build" 2>&1 | tee "${work}/evidence/build.log"
echo "== kmdf_fx2 (best effort)"
bash "${src}/kmdf-fx2.sh" "${work}/build" 2>&1 | tee "${work}/evidence/kmdf-fx2.log" || true
mkdir -p "${work}/evidence/kmdf-fx2"
cp "${work}"/build/kmdf-fx2/*.txt "${work}"/build/kmdf-fx2/*.log "${work}/evidence/kmdf-fx2/" 2>/dev/null || true
cp "${work}/build/imports.txt" "${work}/evidence/" 2>/dev/null || true
chmod -R a+rX "${work}/build"

echo "== harness"
python3 -I "${src}/harness.py" --build "${work}/build" --evidence "${work}/evidence" --user tur \
    --label "Fedora 45 container, $(wine --version 2>/dev/null)" ${TWD_ONLY:+--only "${TWD_ONLY}"}
