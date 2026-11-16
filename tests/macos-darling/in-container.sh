#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Inside the base-image container (ci-run.sh): the tools the test drives the
# desktop with, a user who is not root, an X server, then integration.py as
# that user. Only the test's own tools are installed: tur-runtime needs
# nothing that is not in the base (7-Zip's 7zz, from Fedora's 7zip, is there;
# its version is recorded below).
set -euo pipefail

dnf -y --setopt=install_weak_deps=False --skip-unavailable install \
    bubblewrap xorg-x11-server-Xvfb xdotool xwininfo ImageMagick zip python3 \
    coreutils findutils tar xz curl > /results/dnf.log 2>&1 \
    || { tail -n 30 /results/dnf.log; exit 1; }
rpm -q 7zip bubblewrap xorg-x11-server-Xvfb xdotool ImageMagick >> /results/dnf.log 2>&1 || true

# Darling where the TUR image has it, put there by the image's own script:
# /usr/lib/tur/darling, read-only to the user, and /usr/lib/tur/darling.json.
digest=$(python3 -I -c 'import json; print(json.load(open("/darling/build-record.json"))["sha256"])')
id=$(python3 -I -c 'import json; print(json.load(open("/darling/build-record.json"))["id"])')
commit=$(python3 -I -c 'import json; print(json.load(open("/darling/build-record.json"))["darling_commit"])')
sh /src/image/darling/darling.sh install /darling/darling-runtime.tar.xz "${digest}" / "${id}" "${commit}"

useradd --create-home --uid 1000 tester 2>/dev/null || useradd --create-home tester
install -d -o tester -m 0700 /run/user/tester
chmod 0755 /results
chown tester /results

runuser -u tester -- env -i \
    HOME=/home/tester USER=tester LOGNAME=tester LANG=C.UTF-8 PATH=/usr/bin:/bin \
    XDG_RUNTIME_DIR=/run/user/tester \
    APPS="${APPS}" MODES="${MODES}" \
    python3 -I /src/tests/macos-darling/integration.py \
        --tur-runtime /work/bin/tur-runtime \
        --darling /darling \
        --oracle /oracle \
        --results /results \
        --expected /src/tests/macos-darling/expected.json \
        $([ "${STRICT:-0}" = 1 ] && echo --strict)
