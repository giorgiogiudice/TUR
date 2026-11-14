#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Entry point of the ATL jobs, as root in an Alpine edge container:
#
#   /src   the repository, read-only
#   /work  apks/ (pinned APKs by SHA-256), locks/ (the lock files to run),
#          probe/ (the probe application); evidence/ is written here
#
# Environment: ATL_TITLE, ATL_SCALE (time-limit factor; above 1 under
# emulation), ATL_LIMIT (run only the first N applications), ATL_PROBE
# (file name of the probe APK in /work/probe), ATL_PROBE_PHASES (the
# probe's phases to run after "basic"; all of them when empty).
#
# Alpine is the one distribution that packages the Android Translation
# Layer with its runtime (art_standalone, bionic_translation); installing
# the packages saves building ART in CI. The versions installed are written
# to evidence/alpine-packages.txt.
set -u

EXP=/src/tests/experiments/android
EV=/work/evidence
mkdir -p "$EV"

echo "https://dl-cdn.alpinelinux.org/alpine/edge/testing" >> /etc/apk/repositories
apk update -q
apk add -q android-translation-layer sway swaybg grim python3 py3-gobject3 dbus at-spi2-core \
	mesa-dri-gallium mesa-egl mesa-gles mesa-gl font-dejavu imagemagick procps-ng coreutils \
	adwaita-icon-theme gsettings-desktop-schemas 2>&1 | tail -n 30
rc=$?
# Wanted, not essential: a failure of one must not stop the run.
for p in wtype wl-clipboard mako xdg-desktop-portal xdg-desktop-portal-gtk libatspi font-noto font-noto-emoji mesa-vulkan-swrast; do
	apk add -q "$p" >/dev/null 2>&1 || echo "optional package not installed: $p" | tee -a "$EV/setup.txt"
done
apk info -v 2>/dev/null | sort > "$EV/alpine-packages.txt"
{
	echo "machine: $(uname -m)  kernel: $(uname -r)"
	grep -E '^(android-translation-layer|art_standalone|bionic_translation|libopensles-standalone|gtk4.0|sway|wlroots|mesa-dri-gallium|webkit2gtk-6.0|at-spi2-core)-[0-9]' "$EV/alpine-packages.txt"
	command -v android-translation-layer || echo "android-translation-layer: NOT INSTALLED (apk add status $rc)"
} | tee -a "$EV/setup.txt"

adduser -D -u 1000 tur 2>/dev/null
chown -R tur /work/evidence
mkdir -p /tmp/xdg-tur && chown tur /tmp/xdg-tur && chmod 700 /tmp/xdg-tur

su tur -c "
	export HOME=/home/tur XDG_RUNTIME_DIR=/tmp/xdg-tur
	export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman
	export SWAYSOCK=/tmp/xdg-tur/sway.sock XDG_CURRENT_DESKTOP=sway XDG_SESSION_TYPE=wayland
	export ATL_TITLE='${ATL_TITLE:-run}' ATL_SCALE='${ATL_SCALE:-1}' ATL_LIMIT='${ATL_LIMIT:-0}' ATL_PROBE='${ATL_PROBE:-}' ATL_PROBE_PHASES='${ATL_PROBE_PHASES:-}'
	exec dbus-run-session -- sh $EXP/atl/session.sh
"
echo "session exit status: $?" | tee -a "$EV/setup.txt"
chmod -R a+rX "$EV" 2>/dev/null
exit 0
