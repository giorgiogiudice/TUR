#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The first process of the layers experiment's virtual machine (see vm.sh):
# mounts what a system needs, runs vm-inner.sh, powers the machine off. The
# root file system is a copy of a Fedora 45 container; there is no systemd
# and no network. Runs in CI only.
export PATH=/usr/sbin:/usr/bin

mount -t devtmpfs dev /dev 2>/dev/null
exec </dev/console >/dev/console 2>&1
mount -t proc proc /proc
mount -t sysfs sys /sys
mkdir -p /dev/pts /dev/shm
mount -t devpts devpts /dev/pts
mount -t tmpfs -o mode=1777 tmpfs /dev/shm
mount -t tmpfs tmpfs /run
mount -t tmpfs -o mode=1777 tmpfs /tmp
mount -t cgroup2 cgroup2 /sys/fs/cgroup 2>/dev/null
for name in fd stdin stdout stderr; do
    [ -e "/dev/${name}" ] && continue
    case "${name}" in
    fd) ln -s /proc/self/fd /dev/fd ;;
    stdin) ln -s /proc/self/fd/0 /dev/stdin ;;
    stdout) ln -s /proc/self/fd/1 /dev/stdout ;;
    stderr) ln -s /proc/self/fd/2 /dev/stderr ;;
    esac
done
hostname tur-layers-vm
ip link set lo up
echo 1 >/proc/sys/kernel/sysrq

echo "tur-vm: kernel $(uname -r), $(nproc) processors, $(grep MemTotal /proc/meminfo | tr -s ' ')"
for module in overlay loop btrfs ntsync; do
    modprobe "${module}" 2>&1 | sed "s/^/tur-vm: modprobe ${module}: /"
done

mkdir -p /work/evidence
bash /src/tests/experiments/layers/vm-inner.sh 2>&1 | tee /work/evidence/vm-inner.txt
echo "tur-vm: finished"
sync
mount -o remount,ro / 2>/dev/null
echo o >/proc/sysrq-trigger
sleep 60
