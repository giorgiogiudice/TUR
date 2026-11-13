#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The run half of the KMDF experiment, inside a throw-away Fedora 45
# container with Fedora 45's own Wine (the workflow's kmdf-run job):
#
#   docker run --privileged --network host -v /dev:/dev -v /sys:/sys \
#       -v /run/udev:/run/udev:ro -v CHECKOUT:/src:ro -v BINARIES:/kmdf:ro \
#       -v WORK:/work fedora:45@sha256:... bash /src/tests/windows-drivers/kmdf/kmdf-run.sh
#
# after virtual-usb/usbip_devices.py has plugged in the loopback and the
# FX2-like device. The harness puts the patched ntoskrnl.exe and the new
# wdfldr.sys over this container's Wine (never over a real system's) and
# runs every Wine process as the unprivileged user "tur".
set -euo pipefail

src=/src/tests/windows-drivers
work=/work
mkdir -p "${work}/evidence"

dnf -y --setopt=install_weak_deps=False install \
    wine acl util-linux python3 findutils procps-ng libusb1 \
    >"${work}/evidence/dnf.log" 2>&1 || { tail -n 50 "${work}/evidence/dnf.log"; exit 1; }
id tur >/dev/null 2>&1 || useradd -m -u 1001 tur
{
    . /etc/os-release
    echo "container: ${PRETTY_NAME}"
    echo "kernel (the runner's): $(uname -r)"
    echo "wine: $(wine --version 2>&1)"
    rpm -q wine wine-core libusb1 | sed 's/^/  /'
    echo "binaries:"
    ls -l /kmdf | sed 's/^/  /'
} | tee "${work}/evidence/versions.txt"

mkdir -p "${work}/build"
cp /kmdf/* "${work}/build/"
chmod -R a+rX "${work}/build"
python3 -I "${src}/kmdf/kmdf_harness.py" --build "${work}/build" --evidence "${work}/evidence" --user tur \
    --fx2-log "${work}/evidence/virtual-usb/fx2.log" ${KMDF_ONLY:+--only "${KMDF_ONLY}"}
