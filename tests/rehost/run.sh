#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The integration test of tur-rehost inside a Fedora 45 container
# (.github/workflows/rehost.yml):
#
#   docker run --privileged --network host --shm-size=2g \
#       -v CHECKOUT:/src:ro -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/rehost/run.sh
#
# /work/bin/tur-rehost is the program the workflow built. This installs
# Fedora's packages (the archive tools, bubblewrap, curl, an X server and
# the libraries Electron needs; podman only when native builds are asked
# for), then runs tests/rehost/integration.py as the unprivileged user
# "tur": the component refuses to run as root. The container is privileged
# only so that an unprivileged user inside it may create user namespaces
# (bubblewrap, and Chromium's own sandbox inside it).
#
# Never run on a development machine (AGENTS.md).
set -uo pipefail

out=/work/out
mkdir -p "${out}"

packages="python3 bsdtar 7zip innoextract msitools xz bubblewrap curl ca-certificates
  xorg-x11-server-Xvfb xdotool ffmpeg-free libatomic
  nss atk at-spi2-atk cups-libs gtk3 libdrm alsa-lib libXcomposite libXdamage libXrandr libXScrnSaver
  libxkbcommon pango libnotify libsecret libxshmfence libXtst libXcursor libXinerama libXi libX11-xcb
  mesa-libgbm mesa-libGL mesa-libEGL mesa-dri-drivers dejavu-sans-fonts fontconfig"
if [ "${REHOST_BUILDS:-false}" = "true" ]; then
    packages="${packages} podman fuse-overlayfs shadow-utils"
fi
echo "== packages"
if ! dnf -y --setopt=install_weak_deps=False install --skip-unavailable ${packages} > "${out}/dnf.log" 2>&1; then
    tail -n 40 "${out}/dnf.log"
    echo "::error::dnf could not install the packages"
    exit 1
fi
rpm -q bsdtar 7zip innoextract msitools bubblewrap curl xdotool > "${out}/versions.txt" 2>&1 || true

echo "== user"
useradd --create-home --uid 1500 tur
install -d -o tur -g tur /work/home /work/runtime "${out}"
chmod 0700 /work/runtime
if [ "${REHOST_BUILDS:-false}" = "true" ]; then
    echo "tur:100000:65536" >> /etc/subuid
    echo "tur:100000:65536" >> /etc/subgid
fi

echo "== X server"
# A plain X server with no window manager: the test counts the windows
# Electron maps. Xvfb's socket is /tmp/.X11-unix/X99, which the
# application sandbox shows the application.
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp -ac > "${out}/xvfb.log" 2>&1 &
sleep 2

echo "== integration"
runuser -u tur -- env -i \
    PATH=/usr/bin:/bin HOME=/work/home XDG_RUNTIME_DIR=/work/runtime DISPLAY=:99 LANG=C.UTF-8 \
    REHOST_BUILDS="${REHOST_BUILDS:-false}" REHOST_ONLY="${REHOST_ONLY:-}" \
    python3 -I /src/tests/rehost/integration.py \
        --program /work/bin/tur-rehost --corpus /src/tests/rehost/corpus.json \
        --data /work/data --downloads /work/downloads --out "${out}"
status=$?
chown -R "$(stat -c %u /work)":"$(stat -c %g /work)" "${out}" 2>/dev/null || true
exit ${status}
