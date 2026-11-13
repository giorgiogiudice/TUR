#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Inside the end-to-end container (run.sh): installs what the test needs from
# Fedora 45, starts an X server, and runs session.sh under a session bus of
# its own. Everything it writes goes to $OUT.
set -eu

out=${OUT:?}
mkdir -p "$out/logs" "$out/shots"

finish() {
    status=$?
    # The artifact is read by the runner's user.
    chown -R "${HOST_UID:-0}:${HOST_GID:-0}" "$out" 2>/dev/null || true
    exit "$status"
}
trap finish EXIT

echo "::group::Fedora packages for the test"
dnf -y --setopt=install_weak_deps=False --setopt=tsflags=nodocs install \
    flatpak xorg-x11-server-Xvfb xwininfo ImageMagick dbus-tools dbus-daemon \
    xcb-util-cursor xcb-util-wm xcb-util-image xcb-util-keysyms xcb-util-renderutil libxkbcommon-x11 \
    procps-ng util-linux curl > "$out/logs/dnf.log" 2>&1 || { tail -n 60 "$out/logs/dnf.log"; exit 1; }
echo "::endgroup::"

{
    . /etc/os-release
    echo "container: $PRETTY_NAME"
    echo "kernel: $(uname -r)"
    echo "flatpak: $(flatpak --version)"
    echo "bubblewrap: $(bwrap --version)"
    echo "curl: $(curl --version | head -n 1)"
    echo "tur-apps: $("$TUR_APPS" --version)"
    echo "tur-catalogue: $("$TUR_CATALOGUE" --version)"
    rpm -q flatpak bubblewrap xorg-x11-server-Xvfb qt6-qtbase qt6-qtdeclarative
} | tee "$out/versions.txt"

export DISPLAY=:99
Xvfb "$DISPLAY" -screen 0 1440x1000x24 -nolisten tcp > "$out/logs/xvfb.log" 2>&1 &
xvfb=$!
for _ in $(seq 50); do
    xwininfo -root > /dev/null 2>&1 && break
    sleep 0.2
done

status=0
dbus-run-session -- sh desktop/ci/centre-e2e/session.sh || status=$?
kill "$xvfb" 2>/dev/null || true
exit "$status"
