#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The part of the layers experiment that has to run on the kernel TUR ships:
# Fedora 45's kernel, booted in a virtual machine (QEMU with KVM) whose root
# file system is a copy of this Fedora 45 container, Wine included. Called by
# container.sh (stage vm), as root, in CI only.
#
# Why a virtual machine: the hosted runner's kernel is Ubuntu's. Whether
# overlayfs accepts case-folded layers, and how it behaves without privilege,
# depends on the kernel version, and the kill criterion of the experiment
# names the image's kernel.
#
# How: Fedora's kernel packages are unpacked without their install scripts
# (no boot loader, no dracut); a small initramfs made here loads the modules
# for a virtio disk and ext4 and hands over to vm-init.sh on the root image;
# vm-inner.sh does the measuring and writes into /work/evidence on that
# image, which is copied out when the machine has powered off. The machine
# has no network.
set -euo pipefail

src=/src/tests/experiments/layers
work=/work
evidence="${work}/evidence/vm"
vm="${work}/vm"
mkdir -p "${evidence}" "${vm}"

not_run() {
    echo "NOT RUN: $*" | tee "${evidence}/not-run.txt"
    exit 0
}

dnf -y --setopt=install_weak_deps=False install qemu-system-x86-core busybox xz zstd gzip cpio iproute \
    >"${evidence}/dnf-vm.log" 2>&1 || { tail -n 40 "${evidence}/dnf-vm.log"; not_run "QEMU could not be installed"; }

[ -c /dev/kvm ] && [ -w /dev/kvm ] || not_run "this runner has no usable /dev/kvm"

echo "== Fedora's kernel, unpacked"
mkdir -p "${vm}/rpms"
(cd "${vm}/rpms" && dnf -y download kernel-core kernel-modules-core kernel-modules >>"${evidence}/dnf-vm.log" 2>&1) \
    || { tail -n 40 "${evidence}/dnf-vm.log"; not_run "the kernel packages could not be downloaded"; }
ls -l "${vm}/rpms" | tee "${evidence}/kernel-packages.txt"
rpm -ivh --nodeps --noscripts "${vm}"/rpms/*.rpm >>"${evidence}/dnf-vm.log" 2>&1 \
    || { tail -n 40 "${evidence}/dnf-vm.log"; not_run "the kernel packages could not be unpacked"; }
kver="$(ls /lib/modules | sort -V | tail -n 1)"
depmod -a "${kver}"
kernel="/lib/modules/${kver}/vmlinuz"
[ -f "${kernel}" ] || not_run "no kernel image at ${kernel}"
{
    echo "kernel in the virtual machine: ${kver}"
    rpm -q kernel-core kernel-modules-core kernel-modules qemu-system-x86-core busybox 2>&1 | sed 's/^/  /'
    grep -E '^CONFIG_(OVERLAY_FS|UNICODE|EXT4_FS|BTRFS_FS|USER_NS|NTSYNC|TMPFS_XATTR)' \
        "/lib/modules/${kver}/config" 2>/dev/null | sed 's/^/  /'
} | tee "${evidence}/vm-versions.txt"

echo "== initramfs"
init="${vm}/initramfs"
rm -rf "${init}"
mkdir -p "${init}/bin" "${init}/dev" "${init}/proc" "${init}/sys" "${init}/newroot" "${init}/mods"
cp "$(command -v busybox)" "${init}/bin/busybox"
mknod "${init}/dev/console" c 5 1
number=0
for module in virtio_pci virtio_blk ext4; do
    modprobe -S "${kver}" --show-depends "${module}" 2>/dev/null | awk '$1 == "insmod" { print $2 }'
done | awk '!seen[$0]++' >"${vm}/modules.txt"
while read -r file; do
    number=$((number + 1))
    name="$(basename "${file}")"
    out="$(printf '%s/mods/%03d-%s.ko' "${init}" "${number}" "${name%%.ko*}")"
    case "${file}" in
    *.xz) xz -dc "${file}" >"${out}" ;;
    *.zst) zstd -dc "${file}" >"${out}" ;;
    *.gz) gzip -dc "${file}" >"${out}" ;;
    *) cp "${file}" "${out}" ;;
    esac
done <"${vm}/modules.txt"
echo "modules in the initramfs: $(ls "${init}/mods" | tr '\n' ' ')" | tee -a "${evidence}/vm-versions.txt"
cat >"${init}/init" <<'INIT'
#!/bin/busybox sh
/bin/busybox mount -t devtmpfs dev /dev
exec </dev/console >/dev/console 2>&1
/bin/busybox mount -t proc proc /proc
/bin/busybox mount -t sysfs sys /sys
for module in /mods/*.ko; do
    /bin/busybox insmod "${module}" || echo "initramfs: insmod ${module} failed"
done
tries=0
while [ ! -b /dev/vda ] && [ "${tries}" -lt 30 ]; do
    /bin/busybox sleep 1
    tries=$((tries + 1))
done
if ! /bin/busybox mount -t ext4 /dev/vda /newroot; then
    echo "initramfs: the root file system could not be mounted"
    /bin/busybox ls -l /dev
    /bin/busybox poweroff -f
fi
exec /bin/busybox switch_root /newroot /vm-init.sh
INIT
chmod 0755 "${init}/init"
(cd "${init}" && find . | cpio -o -H newc 2>/dev/null | gzip -1 >"${vm}/initramfs.img")

echo "== root image: a copy of this container"
truncate -s 48G "${vm}/root.img"
mkfs.ext4 -q -F -L tur-vm-root "${vm}/root.img"
mkdir -p /mnt/vmroot
mount -o loop "${vm}/root.img" /mnt/vmroot
cp -ax / /mnt/vmroot/
mkdir -p /mnt/vmroot/proc /mnt/vmroot/sys /mnt/vmroot/dev /mnt/vmroot/run /mnt/vmroot/tmp \
    /mnt/vmroot/src /mnt/vmroot/work/evidence /mnt/vmroot/work/downloads
cp -a /src/tests /mnt/vmroot/src/
cp -a "${work}/downloads/." /mnt/vmroot/work/downloads/ 2>/dev/null || true
chown -R 1001:1001 /mnt/vmroot/work
install -m 0755 "${src}/vm-init.sh" /mnt/vmroot/vm-init.sh
df -h /mnt/vmroot | tail -n 1
umount /mnt/vmroot

echo "== virtual machine"
cpus="$(nproc)"
status=0
timeout 55m qemu-system-x86_64 -machine q35,accel=kvm -cpu host -smp "${cpus}" -m 11G \
    -kernel "${kernel}" -initrd "${vm}/initramfs.img" \
    -append "console=ttyS0 panic=-1 selinux=0 audit=0 loglevel=4" \
    -drive "file=${vm}/root.img,format=raw,if=virtio,cache=unsafe" \
    -nic none -nographic -no-reboot </dev/null | tee "${evidence}/qemu-console.txt" || status=$?
echo "QEMU ended with status ${status}" | tee -a "${evidence}/vm-versions.txt"

echo "== evidence out of the root image"
mount -o loop "${vm}/root.img" /mnt/vmroot
cp -a /mnt/vmroot/work/evidence/. "${evidence}/" 2>/dev/null || true
umount /mnt/vmroot
rm -f "${vm}/root.img"
ls -la "${evidence}" | head -n 60
