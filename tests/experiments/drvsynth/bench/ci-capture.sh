#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The run half of the driver-synthesis test bench, in a throw-away Fedora 45
# container with Fedora 45's own Wine (workflow exp-drvsynth, job
# "capture"):
#
#   docker run --privileged --network host -v /dev:/dev -v /sys:/sys \
#       -v /run/udev:/run/udev:ro -v CHECKOUT:/src:ro -v BUILD:/build:ro \
#       -v RUN_DIR:/run/drvsynth -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/experiments/drvsynth/bench/ci-capture.sh
#
# after the emulated device has been plugged in on the runner and usbmon
# loaded. Writes /work/handoff (captures and timelines) and /work/evidence.
set -euo pipefail

work=/work
mkdir -p "${work}/evidence" "${work}/handoff"
dnf -y --setopt=install_weak_deps=False install \
    wine acl util-linux python3 findutils procps-ng libusb1 wireshark-cli \
    >"${work}/evidence/dnf.log" 2>&1 || { tail -n 50 "${work}/evidence/dnf.log"; exit 1; }
id tur >/dev/null 2>&1 || useradd -m -u 1001 tur
{
    . /etc/os-release
    echo "container: ${PRETTY_NAME}"
    echo "kernel (the runner's): $(uname -r)"
    echo "wine: $(wine --version 2>&1)"
    rpm -q wine wine-core libusb1 wireshark-cli | sed 's/^/  /'
    echo "binaries:"
    ls -l /build | sed 's/^/  /'
    echo "device: $(cat /run/drvsynth/ready)"
} | tee "${work}/evidence/versions.txt"

mkdir -p "${work}/build"
cp /build/* "${work}/build/"
chmod -R a+rX "${work}/build"
python3 -I /src/tests/experiments/drvsynth/bench/sessions.py --build "${work}/build" \
    --evidence "${work}/evidence" --out "${work}/handoff" --run-dir /run/drvsynth --user tur
