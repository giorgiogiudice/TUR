#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# windiff under Fedora 45's own Wine (the Wine TUR would ship), in a Fedora
# 45 container:
#
#   docker run -v CHECKOUT:/src:ro -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/experiments/windiff/ci/wine-fedora.sh
#
# /work/bin holds the check programs (the build job's artifact). Writes
# /work/evidence/results-fedora.json and logs.
set -uo pipefail
. /src/tests/experiments/windiff/ci/wine-common.sh

ev=/work/evidence
mkdir -p "${ev}"
dnf -y --setopt=install_weak_deps=False install wine python3 util-linux findutils procps-ng tar gzip \
    >"${ev}/dnf-fedora.log" 2>&1 || { tail -n 40 "${ev}/dnf-fedora.log"; exit 1; }
windiff_user
{
    . /etc/os-release
    echo "container: ${PRETTY_NAME}"
    echo "kernel (the runner's): $(uname -r)"
    echo "wine: $(wine --version 2>&1)"
    rpm -q wine wine-core | sed 's/^/  /'
    echo "file system of /work: $(stat -f -c %T /work)"
} | tee "${ev}/versions-fedora.txt"

run_checks fedora "$(command -v wine)" "$(command -v wineserver)" "${ev}"
chmod -R a+rX "${ev}"
[ -s "${ev}/results-fedora.json" ]
