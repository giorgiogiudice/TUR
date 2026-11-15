#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# What the layers experiment measures inside its virtual machine, on Fedora
# 45's kernel with Fedora 45's Wine (see vm.sh and vm-init.sh). As root: the
# file systems and the kernel's messages. As the user "tur": everything else.
set -uo pipefail

src=/src/tests/experiments/layers
work=/work
evidence="${work}/evidence"

as_tur() {
    setpriv --reuid=1001 --regid=1001 --init-groups \
        env HOME=/home/tur USER=tur LOGNAME=tur PATH=/usr/bin:/usr/sbin LANG=C.UTF-8 "$@"
}

{
    echo "kernel: $(uname -r)"
    echo "wine: $(wine --version 2>&1)"
    echo "bubblewrap: $(bwrap --version 2>&1)"
    echo "overlay module parameters:"
    for file in /sys/module/overlay/parameters/*; do
        echo "  $(basename "${file}") = $(cat "${file}")"
    done
    echo "ext4 casefold support: $(cat /sys/fs/ext4/features/casefold 2>&1)"
    echo "unprivileged user namespaces: user.max_user_namespaces = $(cat /proc/sys/user/max_user_namespaces)"
    echo "ntsync device: $(ls -l /dev/ntsync 2>&1)"
} | tee "${evidence}/vm-environment.txt"

echo "== file systems"
bash "${src}/fs-setup.sh" "${work}/fs" 1001:1001 2>&1 | tee "${evidence}/file-systems.txt"
chown 1001:1001 "${work}" "${evidence}"

echo "== file-system matrix on this kernel"
python3 -I "${src}/fsmatrix.py" --fs-root "${work}/fs" --user tur --out "${evidence}/fsmatrix.json" \
    || echo "the matrix failed"

echo "== Wine on layers on this kernel"
mkdir -p /tmp/.X11-unix
chmod 1777 /tmp/.X11-unix
as_tur python3 -I "${src}/experiment.py" --work "${work}" --evidence "${evidence}" \
    --apps /src/tests/windows-runtime/apps.json --phases preflight,h1,mini,safety \
    || echo "the experiment failed"

dmesg | grep -iE 'overlay|casefold|ext4|btrfs' | tail -n 80 >"${evidence}/vm-dmesg.txt"
chmod -R a+rX "${evidence}"
df -h "${work}" "${work}"/fs/* | tee "${evidence}/vm-disk.txt"
