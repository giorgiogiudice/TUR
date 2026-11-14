#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The winsee experiment inside a Fedora 45 container, with Fedora 45's own
# Wine (the Wine TUR would ship). Started by .github/workflows/exp-winsee.yml:
#
#   docker run -v CHECKOUT:/src:ro -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/experiments/winsee/run-in-fedora.sh
#
# WORK/downloads holds the applications (fetched and checked on the runner),
# WORK/hostbin the programs built there (tur-pax and winsee-decide), if they
# built. Everything measured is written to WORK/evidence. Wine, the
# applications and every harness run as the ordinary user "tur".
#
#   1. bridge/build.sh       tur-winbridge.exe, tur-winhook.dll, the test windows
#   2. harness/winsee.py     hypothesis A: the tiers on real applications
#   3. harness/linux_labels.py  labelled windows from the accessibility tree
#   4. harness/policy.py     the Windows path under the real task loop
#
# The script ends with status 0 when the harnesses ran: what they found is
# in the evidence, a negative result included.
set -uo pipefail

src=/src/tests/experiments/winsee
work=/work
evidence="${work}/evidence"
mkdir -p "${evidence}/logs" "${work}/build" "${work}/run"

echo "== packages"
dnf -y --setopt=install_weak_deps=False --setopt=tsflags=nodocs install \
    wine mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt \
    xorg-x11-server-Xvfb python3 python3-pillow python3-gobject \
    gtk3 at-spi2-core at-spi2-atk dbus-daemon dbus-tools glib2 \
    firefox qt6-qtdeclarative qt6-qtbase-gui \
    dejavu-sans-fonts liberation-sans-fonts util-linux procps-ng findutils \
    >"${evidence}/logs/dnf.log" 2>&1 || { tail -n 60 "${evidence}/logs/dnf.log"; echo "::error::the packages did not install"; exit 1; }
# Wanted, not needed: a window manager, a real pointer, the qml runtime.
for package in openbox xdotool xsetroot ImageMagick qt6-qtdeclarative-devel; do
    dnf -y --setopt=install_weak_deps=False --setopt=tsflags=nodocs install "${package}" >>"${evidence}/logs/dnf-optional.log" 2>&1 ||
        echo "optional package ${package} was not installed"
done

id tur >/dev/null 2>&1 || useradd -m -u 1001 tur

{
    . /etc/os-release
    echo "container: ${PRETTY_NAME}"
    echo "kernel (the runner's): $(uname -r)"
    echo "processors: $(nproc); memory: $(awk '/MemTotal/ {print $2 " kB"}' /proc/meminfo)"
    echo "wine: $(wine --version 2>&1)"
    echo "packages:"
    rpm -q wine wine-core mingw64-gcc mingw64-headers mingw64-crt xorg-x11-server-Xvfb openbox xdotool gtk3 at-spi2-core \
        at-spi2-atk firefox qt6-qtdeclarative qt6-qtbase python3 python3-pillow 2>&1 | sed 's/^/  /'
    echo "wine's accessibility libraries:"
    find /usr/lib64/wine /usr/lib/wine -name 'oleacc.dll' -o -name 'uiautomationcore.dll' 2>/dev/null | sed 's/^/  /'
    echo "qml runtime: $(command -v qml6 qml-qt6 qml 2>/dev/null | head -n 1) $(ls /usr/lib64/qt6/bin/qml 2>/dev/null)"
} | tee "${evidence}/versions.txt"

echo "== build"
bash "${src}/bridge/build.sh" "${work}/build" 2>&1 | tee "${evidence}/logs/build.log"
mkdir -p "${evidence}/build"
cp "${work}/build/build.json" "${work}/build/imports.txt" "${evidence}/build/" 2>/dev/null || true
cp -r "${work}/build/logs" "${evidence}/build/logs" 2>/dev/null || true
chmod -R a+rX "${work}/build"
chown -R tur:tur "${work}/run" "${evidence}"

as_tur() {
    setpriv --reuid=tur --regid=tur --init-groups -- env -i \
        HOME=/home/tur USER=tur LOGNAME=tur PATH=/usr/bin:/bin LANG=C.UTF-8 LC_ALL=C.UTF-8 \
        XDG_RUNTIME_DIR=/tmp/tur-runtime PYTHONUNBUFFERED=1 "$@"
}
mkdir -p /tmp/tur-runtime && chown tur:tur /tmp/tur-runtime && chmod 0700 /tmp/tur-runtime
# The X servers the harnesses start are not root's.
mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix

if [ -f "${work}/build/tur-winbridge.exe" ]; then
    echo "== hypothesis A: the bridge on real applications"
    as_tur python3 "${src}/harness/winsee.py" --build "${work}/build" --downloads "${work}/downloads" \
        --work "${work}/run" --evidence "${evidence}" ${WINSEE_ONLY:+--only "${WINSEE_ONLY}"} 2>&1 | tee "${evidence}/logs/winsee.log"
    echo "winsee.py ended with status ${PIPESTATUS[0]}"
else
    echo "::error::tur-winbridge.exe did not build: hypothesis A was NOT RUN"
    echo "tur-winbridge.exe did not build: hypothesis A was NOT RUN" >"${evidence}/results.md"
fi

echo "== labelled windows from the accessibility tree"
as_tur dbus-run-session -- python3 "${src}/harness/linux_labels.py" --evidence "${evidence}" --repository /src 2>&1 |
    tee "${evidence}/logs/linux-labels.log"
echo "linux_labels.py ended with status ${PIPESTATUS[0]}"

echo "== the Windows path under the real task loop"
if [ -x "${work}/hostbin/tur-pax" ] && [ -x "${work}/hostbin/winsee-decide" ] && [ -f "${work}/build/tur-winbridge.exe" ]; then
    as_tur dbus-run-session -- python3 "${src}/harness/policy.py" --tur-pax "${work}/hostbin/tur-pax" \
        --decide "${work}/hostbin/winsee-decide" --build "${work}/build" --work "${work}/run" \
        --evidence "${evidence}" --repository /src 2>&1 | tee "${evidence}/logs/policy.log"
    echo "policy.py ended with status ${PIPESTATUS[0]}"
else
    mkdir -p "${evidence}/policy"
    echo "### winsee: the Windows path under the real task loop: NOT RUN (tur-pax, winsee-decide or the bridge did not build)" |
        tee "${evidence}/policy/policy.md"
fi

ls -la "${evidence}" "${evidence}/dataset" 2>/dev/null | head -n 120
exit 0
