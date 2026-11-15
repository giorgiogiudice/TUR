#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The layers experiment inside a Fedora container, with that Fedora's own
# Wine. Started by .github/workflows/exp-layers.yml as
#
#   docker run --rm --privileged --network host -v /dev:/dev \
#       -v CHECKOUT:/src:ro -v WORK:/work fedora:NN@sha256:... \
#       bash /src/tests/experiments/layers/container.sh STAGE
#
#   old   the older Wine (Fedora 43's): a base layer, the five applications
#         installed on layers over it, a plain prefix to update later
#   new   the newer Wine (Fedora 45's, the Wine TUR ships): everything else,
#         on the layers the old stage left under /work
#   vm    Fedora 45 again, but the measurements run in a virtual machine on
#         Fedora 45's kernel (vm.sh): the runner's kernel is Ubuntu's
#
# The container is privileged so that it can read the kernel log, use loop
# devices and (vm) KVM. Wine, every Windows program and every mount of a
# layer run as the unprivileged user "tur"; packages are Fedora's only.
set -euo pipefail

stage="${1:?stage required: old, new or vm}"
src=/src/tests/experiments/layers
work=/work
evidence="${work}/evidence"
mkdir -p "${evidence}/${stage}"

packages=(wine bubblewrap util-linux python3 unzip e2fsprogs btrfs-progs attr file findutils procps-ng
    xorg-x11-server-Xvfb kmod)
if [ "${stage}" = new ]; then
    packages+=(mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt)
fi
dnf -y --setopt=install_weak_deps=False install "${packages[@]}" \
    >"${evidence}/${stage}/dnf.log" 2>&1 || { tail -n 60 "${evidence}/${stage}/dnf.log"; exit 1; }
# The window manager and the tools the application checks look at windows
# with, fonts and a software OpenGL, where the release has them under these
# names. What is missing shows in the versions below and in the results.
dnf -y --setopt=install_weak_deps=False install --skip-unavailable openbox wmctrl xwininfo ImageMagick \
    wine-fonts dejavu-sans-fonts liberation-sans-fonts mesa-dri-drivers mesa-libGL \
    >>"${evidence}/${stage}/dnf.log" 2>&1 || true
# An X server that is not root cannot make this directory itself.
mkdir -p /tmp/.X11-unix
chmod 1777 /tmp/.X11-unix

id tur >/dev/null 2>&1 || useradd -m -u 1001 tur
as_tur() {
    setpriv --reuid=1001 --regid=1001 --init-groups \
        env HOME=/home/tur USER=tur LOGNAME=tur PATH=/usr/bin:/usr/sbin LANG=C.UTF-8 "$@"
}

{
    . /etc/os-release
    echo "stage: ${stage}"
    echo "container: ${PRETTY_NAME}"
    echo "kernel (the runner's): $(uname -r)"
    echo "wine: $(wine --version 2>&1)"
    echo "bubblewrap: $(bwrap --version 2>&1) ($(stat -c '%A' "$(command -v bwrap)"))"
    echo "util-linux: $(mount --version 2>&1 | head -n 1)"
    echo "python: $(python3 --version 2>&1)"
    echo "packages:"
    rpm -q wine wine-core wine-common bubblewrap e2fsprogs btrfs-progs xorg-x11-server-Xvfb openbox wmctrl \
        xwininfo ImageMagick mesa-dri-drivers mingw64-gcc 2>&1 | sed 's/^/  /' || true
    echo "wine builds for: $(ls -d /usr/lib64/wine/* /usr/lib/wine/* 2>/dev/null | tr '\n' ' ')"
    echo "ntsync device: $(ls -l /dev/ntsync 2>&1)"
    echo "disk:"
    df -h "${work}" "${work}"/fs/* 2>/dev/null | sed 's/^/  /'
} | tee "${evidence}/${stage}/versions.txt"

chown tur "${work}" "${evidence}" "${evidence}/${stage}"

echo "== the registry module's own test"
as_tur python3 -I "${src}/winereg.py" --self-test | tee "${evidence}/${stage}/winereg-self-test.txt"

experiment() {
    as_tur python3 -I "${src}/experiment.py" --work "${work}" --evidence "${evidence}" \
        --apps /src/tests/windows-runtime/apps.json --build "${work}/build" "$@"
}

case "${stage}" in
old)
    experiment --phases preflight,old-base
    cp "${evidence}/results.json" "${evidence}/old/results-after-old.json" 2>/dev/null || true
    ;;
new)
    echo "== test programs for the runtime layer"
    bash "${src}/rt/build.sh" "${work}/build" 2>&1 | tee "${evidence}/new/build.txt" || true
    chmod -R a+rX "${work}/build" 2>/dev/null || true
    echo "== file-system matrix (the runner's kernel)"
    python3 -I "${src}/fsmatrix.py" --fs-root "${work}/fs" --user tur \
        --out "${evidence}/fsmatrix-runner-kernel.json" || echo "the matrix failed"
    experiment --phases preflight,h1,update,rebase,layered,plain,disk,groups,safety,extras,remove-layered
    ;;
vm)
    echo "== installers, fetched here because the virtual machine has no network"
    experiment --phases download --only 7zip-exe,notepadpp-nsis
    bash "${src}/vm.sh"
    ;;
*)
    echo "unknown stage ${stage}" >&2
    exit 2
    ;;
esac

chmod -R a+rX "${evidence}" 2>/dev/null || true
df -h "${work}" 2>/dev/null | tail -n 1
